//! Source connector decision and exact row counts.
//!
//! Counts use a read-only TLS session. They are not the gateway SELECT executor.

use std::fs;
use std::path::Path;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use rustls::ClientConfig;
use rustls::RootCertStore;
use sha2::{Digest, Sha256};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::config::Config;

/// Protocol constant of `oo-connectors`. A non-PostgreSQL type is unsupported.
pub const CONNECTOR_PROTOCOL: &str = "oo.source.pgsql/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectorDecision {
    pub supported: bool,
    pub protocol: &'static str,
}

pub fn connector_decision(database_type: &str) -> ConnectorDecision {
    let supported = matches!(database_type, "postgresql" | "postgres");
    ConnectorDecision { supported, protocol: CONNECTOR_PROTOCOL }
}

#[cfg(test)]
mod tests {
    use super::{connector_decision, CONNECTOR_PROTOCOL};

    #[test]
    fn only_postgresql_uses_the_connector_protocol() {
        assert!(connector_decision("postgresql").supported);
        assert!(connector_decision("postgres").supported);
        assert!(!connector_decision("mysql").supported);
        assert!(!connector_decision("sqlite").supported);
        assert_eq!(connector_decision("mysql").protocol, CONNECTOR_PROTOCOL);
        assert_eq!(CONNECTOR_PROTOCOL, "oo.source.pgsql/v1");
    }

    #[test]
    fn source_preimage_names_the_configured_tables() {
        let names = vec!["finance.ledger".to_owned()];
        let identity = super::SourceIdentity {
            database_type: "postgresql",
            host: "db.internal",
            port: 5432,
            database: "app",
            username: "reader",
            secret_ref: "secret://pg/app",
        };
        let body = super::source_preimage(&identity, &names, &[4], "17.10");
        assert!(body.contains("finance.ledger=4\n"));
        assert!(!body.contains("\nparty="));
        assert!(!body.contains("\naccount="));
        assert!(!body.contains("\nentry="));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceCounts {
    pub rows: Vec<i64>,
    pub version: String,
    pub source_sha256: String,
}

pub fn observe_database(config: &Config) -> Result<(), &'static str> {
    if !connector_decision(&config.database_type).supported {
        return Err("unsupported");
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "runtime")?;
    runtime.block_on(observe_task(config))
}

async fn observe_task(config: &Config) -> Result<(), &'static str> {
    let password = read_secret(&config.password_file, 4096)?;
    let ca = fs::read(&config.ca_file).map_err(|_| "tls")?;
    let connector = rustls_connector(&ca)?;
    let mut cfg = tokio_postgres::Config::new();
    cfg.host(&config.host);
    cfg.port(config.port);
    cfg.dbname(&config.database);
    cfg.user(&config.username);
    cfg.password(password);
    cfg.ssl_mode(tokio_postgres::config::SslMode::Require);
    cfg.application_name("minimal-product-loop-close");
    cfg.connect_timeout(Duration::from_secs(2));
    let (client, connection) = cfg.connect(connector).await.map_err(|_| "connect")?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let name: String = client
        .query_one("SELECT current_database()", &[])
        .await
        .map_err(|_| "database")?
        .get(0);
    if name == config.database { Ok(()) } else { Err("database") }
}

pub fn count_source(config: &Config) -> Result<SourceCounts, &'static str> {
    if !connector_decision(&config.database_type).supported {
        return Err("unsupported");
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "runtime")?;
    runtime.block_on(count_task(config))
}

async fn count_task(config: &Config) -> Result<SourceCounts, &'static str> {
    let password = read_secret(&config.password_file, 4096)?;
    let ca = fs::read(&config.ca_file).map_err(|_| "tls")?;
    let connector = rustls_connector(&ca)?;
    let mut cfg = tokio_postgres::Config::new();
    cfg.host(&config.host);
    cfg.port(config.port);
    cfg.dbname(&config.database);
    cfg.user(&config.username);
    cfg.password(password);
    cfg.ssl_mode(tokio_postgres::config::SslMode::Require);
    cfg.application_name("minimal-product-loop");
    cfg.connect_timeout(Duration::from_secs(30));
    let (client, connection) = cfg.connect(connector).await.map_err(|_| "connect")?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client.batch_execute("SET statement_timeout = '30s'").await.map_err(|_| "timeout")?;
    let version: String = client.query_one("SELECT current_setting('server_version')", &[]).await.map_err(|_| "version")?.get(0);
    if config.tables.is_empty() || config.tables.len() > 16 {
        return Err("tables");
    }
    let mut names = Vec::with_capacity(config.tables.len());
    let mut rows = Vec::with_capacity(config.tables.len());
    for (schema, table) in &config.tables {
        names.push(format!("{schema}.{table}"));
        rows.push(count_table(&client, schema, table).await?);
    }
    let source_sha256 = source_hash(config, &names, &rows, &version);
    Ok(SourceCounts { rows, source_sha256, version })
}

async fn count_table(client: &tokio_postgres::Client, schema: &str, table: &str) -> Result<i64, &'static str> {
    if !ident_ok(schema) || !ident_ok(table) {
        return Err("ident");
    }
    let sql = format!("SELECT count(*)::int8 FROM \"{schema}\".\"{table}\"");
    let row = client.query_one(&sql, &[]).await.map_err(|_| "count")?;
    row.try_get(0).map_err(|_| "count")
}

pub fn source_hash(config: &Config, names: &[String], rows: &[i64], version: &str) -> String {
    let body = source_preimage(
        &SourceIdentity {
            database_type: &config.database_type,
            host: &config.host,
            port: config.port,
            database: &config.database,
            username: &config.username,
            secret_ref: &config.secret_ref,
        },
        names,
        rows,
        version,
    );
    hex(body.as_bytes())
}

pub(crate) struct SourceIdentity<'a> {
    database_type: &'a str,
    host: &'a str,
    port: u16,
    database: &'a str,
    username: &'a str,
    secret_ref: &'a str,
}

pub(crate) fn source_preimage(identity: &SourceIdentity<'_>, names: &[String], rows: &[i64], version: &str) -> String {
    let mut body = format!(
        "oo.minimal-product-loop.source/v1\ntype={}\nhost={}\nport={}\ndatabase={}\nusername={}\nsecret_ref={}\nserver_version={version}\n",
        identity.database_type, identity.host, identity.port, identity.database, identity.username, identity.secret_ref
    );
    for (name, count) in names.iter().zip(rows) {
        body.push_str(name);
        body.push('=');
        body.push_str(&count.to_string());
        body.push('\n');
    }
    body
}

pub fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub(crate) fn quote_ident(value: &str) -> Result<String, &'static str> {
    if ident_ok(value) {
        Ok(format!("\"{value}\""))
    } else {
        Err("ident")
    }
}

fn ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let first = bytes[0];
    if first != b'_' && !first.is_ascii_lowercase() {
        return false;
    }
    bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

/// One logical column on a sealed physical relation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveColumn {
    pub name: String,
    pub physical_name: String,
    pub type_name: String,
    pub typmod: i32,
    pub type_oid: i64,
}

/// One sealed table and the physical relation the reader can count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveTable {
    pub logical_schema: String,
    pub logical_table: String,
    pub physical_schema: String,
    pub physical_table: String,
    pub rows: i64,
    pub columns: Vec<LiveColumn>,
}

/// Mapping digest, server version, and the policy role shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingRead {
    pub config_digest: String,
    pub version: String,
    pub role_state: String,
    pub tables: Vec<LiveTable>,
}

pub fn bulk_source_hash(config: &Config, names: &[String], rows: &[i64], version: &str, import_hash: &str) -> String {
    let mut body = format!(
        "oo.minimal-product-loop.source/v1\ntype={}\nhost={}\nport={}\ndatabase={}\nusername={}\nsecret_ref={}\nimport_hash={import_hash}\nserver_version={version}\n",
        config.database_type, config.host, config.port, config.database, config.username, config.secret_ref
    );
    for (name, count) in names.iter().zip(rows) {
        body.push_str(name);
        body.push('=');
        body.push_str(&count.to_string());
        body.push('\n');
    }
    hex(body.as_bytes())
}

pub fn read_sealed_mapping(
    config: &Config,
    source_id: &str,
    generation: i64,
    logical: &[(String, String)],
    role: &str,
) -> Result<MappingRead, &'static str> {
    if !connector_decision(&config.database_type).supported {
        return Err("unsupported");
    }
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| "runtime")?;
    runtime.block_on(mapping_task(config, source_id, generation, logical, role))
}

async fn mapping_task(
    config: &Config,
    source_id: &str,
    generation: i64,
    logical: &[(String, String)],
    role: &str,
) -> Result<MappingRead, &'static str> {
    let client = connect(config, "minimal-product-loop", Duration::from_secs(30)).await?;
    client.batch_execute("SET statement_timeout = '30s'").await.map_err(|_| "timeout")?;
    let version: String = client.query_one("SELECT current_setting('server_version')", &[]).await.map_err(|_| "version")?.get(0);
    let mut digest = String::new();
    let mut tables = Vec::new();
    for (schema, table) in logical {
        let found = client
            .query(
                "SELECT DISTINCT config_digest FROM _oo.physical_mappings WHERE source = $1 AND run_generation = $2 AND logical_namespace = $3 AND logical_table = $4 AND level = 't'",
                &[&source_id, &generation, schema, table],
            )
            .await
            .map_err(mapping_code)?;
        if found.len() != 1 {
            return Err("mapping");
        }
        let next: String = found[0].get(0);
        if digest.is_empty() {
            digest = next;
        } else if digest != next {
            return Err("mapping");
        }
        tables.push(read_live_table(&client, source_id, &digest, generation, schema, table).await?);
    }
    let role_state = role_state(&client, role).await?;
    Ok(MappingRead { config_digest: digest, version, role_state, tables })
}

async fn read_live_table(
    client: &tokio_postgres::Client,
    source_id: &str,
    digest: &str,
    generation: i64,
    schema: &str,
    table: &str,
) -> Result<LiveTable, &'static str> {
    let physical_rows = client
        .query(
            "SELECT physical_name FROM _oo.physical_mappings WHERE source = $1 AND config_digest = $2 AND run_generation = $3 AND level = 't' AND logical_namespace = $4 AND logical_table = $5",
            &[&source_id, &digest, &generation, &schema, &table],
        )
        .await
        .map_err(mapping_code)?;
    if physical_rows.len() != 1 {
        return Err("mapping");
    }
    let physical_table: String = physical_rows[0].get(0);
    if !ident_ok("_oo") || !ident_ok(&physical_table) {
        return Err("mapping");
    }
    let rows = count_table(client, "_oo", &physical_table).await?;
    let column_rows = client
        .query(
            "SELECT m.logical_column, m.physical_name, t.typname, a.atttypmod, a.atttypid::int8 \
             FROM _oo.physical_mappings AS m \
             JOIN pg_catalog.pg_namespace AS n ON n.nspname = '_oo' \
             JOIN pg_catalog.pg_class AS c ON c.relnamespace = n.oid AND c.relname = $6 \
             JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid AND a.attname = m.physical_name AND a.attnum > 0 AND NOT a.attisdropped \
             JOIN pg_catalog.pg_type AS t ON t.oid = a.atttypid \
             WHERE m.source = $1 AND m.config_digest = $2 AND m.run_generation = $3 AND m.level = 'c' AND m.logical_namespace = $4 AND m.logical_table = $5 \
             ORDER BY a.attnum",
            &[&source_id, &digest, &generation, &schema, &table, &physical_table],
        )
        .await
        .map_err(mapping_code)?;
    let mut columns = Vec::new();
    for row in column_rows {
        let physical_name: String = row.get(1);
        if !ident_ok(&physical_name) || physical_name == "_oo_pos" {
            return Err("mapping");
        }
        columns.push(LiveColumn {
            name: row.get(0),
            physical_name,
            type_name: row.get(2),
            typmod: row.get(3),
            type_oid: row.get(4),
        });
    }
    if columns.is_empty() {
        return Err("mapping");
    }
    Ok(LiveTable {
        logical_schema: schema.to_owned(),
        logical_table: table.to_owned(),
        physical_schema: "_oo".to_owned(),
        physical_table,
        rows,
        columns,
    })
}

async fn role_state(client: &tokio_postgres::Client, role: &str) -> Result<String, &'static str> {
    let rows = client
        .query(
            "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, rolbypassrls, rolreplication FROM pg_catalog.pg_roles WHERE rolname = $1",
            &[&role],
        )
        .await
        .map_err(|_| "mapping")?;
    if rows.is_empty() {
        return Ok("absent".to_owned());
    }
    if rows.len() != 1 {
        return Err("mapping");
    }
    let flags: [bool; 7] = [rows[0].get(0), rows[0].get(1), rows[0].get(2), rows[0].get(3), rows[0].get(4), rows[0].get(5), rows[0].get(6)];
    if flags.iter().any(|flag| *flag) {
        Ok("mismatch".to_owned())
    } else {
        Ok("present".to_owned())
    }
}

fn mapping_code(error: tokio_postgres::Error) -> &'static str {
    match error.code().map(|code| code.code()) {
        Some("42501") => "mapping.denied",
        Some("42P01") => "mapping.absent",
        _ => "mapping",
    }
}

pub(crate) async fn connect(config: &Config, application: &str, timeout: Duration) -> Result<tokio_postgres::Client, &'static str> {
    connect_as(config, &config.username, &config.password_file, &config.ca_file, application, timeout).await
}

pub(crate) fn dev_role_statements(role: &str, login: &str) -> Result<Vec<String>, &'static str> {
    let role_sql = quote_ident(role)?;
    let login_sql = quote_ident(login)?;
    Ok(vec![
        format!("CREATE ROLE {role_sql} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION"),
        format!("GRANT {role_sql} TO {login_sql} WITH INHERIT FALSE, SET TRUE"),
    ])
}

pub(crate) fn mapping_reader_statements(reader: &str) -> Result<Vec<String>, &'static str> {
    let reader_sql = quote_ident(reader)?;
    Ok(vec![
        format!("GRANT SELECT ON TABLE \"_oo\".physical_mappings TO {reader_sql}"),
        format!("GRANT EXECUTE ON FUNCTION \"_oo\".lookup_physical_mapping(text, text, bigint, text, text, text, text) TO {reader_sql}"),
    ])
}

pub(crate) fn apply_admin_statements(config: &Config, admin_user: &str, admin_password: &Path, admin_ca: &Path, statements: &[String]) -> Result<(), &'static str> {
    grant_statements_ok(statements)?;
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| "runtime")?;
    runtime.block_on(admin_task(config, admin_user, admin_password, admin_ca, statements))
}

pub(crate) fn apply_role_grants(config: &Config, admin_user: &str, admin_password: &Path, admin_ca: &Path, statements: &[String]) -> Result<(), &'static str> {
    grant_statements_ok(statements)?;
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| "runtime")?;
    runtime.block_on(grant_task(config, admin_user, admin_password, admin_ca, statements))
}

fn grant_statements_ok(statements: &[String]) -> Result<(), &'static str> {
    if statements.is_empty() || statements.len() > 16 {
        return Err("role.grant");
    }
    for statement in statements {
        if statement.is_empty() || statement.len() > 4096 || statement.contains('\0') || statement.contains(';') {
            return Err("role.grant");
        }
    }
    Ok(())
}

async fn admin_task(config: &Config, admin_user: &str, admin_password: &Path, admin_ca: &Path, statements: &[String]) -> Result<(), &'static str> {
    let mut client = connect_as(config, admin_user, admin_password, admin_ca, "minimal-product-loop-admin", Duration::from_secs(30)).await?;
    client.batch_execute("SET statement_timeout = '30s'").await.map_err(|_| "timeout")?;
    let transaction = client.transaction().await.map_err(grant_code)?;
    for statement in statements {
        transaction.batch_execute(statement).await.map_err(grant_code)?;
    }
    transaction.commit().await.map_err(grant_code)?;
    Ok(())
}

async fn grant_task(config: &Config, admin_user: &str, admin_password: &Path, admin_ca: &Path, statements: &[String]) -> Result<(), &'static str> {
    let mut client = connect_as(config, admin_user, admin_password, admin_ca, "minimal-product-loop-grant", Duration::from_secs(30)).await?;
    client.batch_execute("SET statement_timeout = '30s'").await.map_err(|_| "timeout")?;
    let transaction = client.transaction().await.map_err(grant_code)?;
    // The copy installer owns mapped tables as oo_target_owner. This grant uses that
    // existing owner for the provision window and does not change the source actor.
    transaction.batch_execute("SET LOCAL ROLE oo_target_owner").await.map_err(grant_code)?;
    for statement in statements {
        transaction.batch_execute(statement).await.map_err(grant_code)?;
    }
    transaction.commit().await.map_err(grant_code)?;
    Ok(())
}

fn grant_code(error: tokio_postgres::Error) -> &'static str {
    match error.code().map(|code| code.code()) {
        Some("42501") => "permission_denied",
        Some("42704") => "role_absent",
        Some("42P01") => "relation_absent",
        _ => "role.grant",
    }
}

async fn connect_as(
    config: &Config,
    user: &str,
    password_file: &Path,
    ca_file: &Path,
    application: &str,
    timeout: Duration,
) -> Result<tokio_postgres::Client, &'static str> {
    let password = read_secret(password_file, 4096)?;
    let ca = fs::read(ca_file).map_err(|_| "tls")?;
    let connector = rustls_connector(&ca)?;
    let mut cfg = tokio_postgres::Config::new();
    cfg.host(&config.host);
    cfg.port(config.port);
    cfg.dbname(&config.database);
    cfg.user(user);
    cfg.password(password);
    cfg.ssl_mode(tokio_postgres::config::SslMode::Require);
    cfg.application_name(application);
    cfg.connect_timeout(timeout);
    let (client, connection) = cfg.connect(connector).await.map_err(|_| "connect")?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

#[cfg(test)]
mod ident_tests {
    use super::ident_ok;

    #[test]
    fn physical_names_may_start_with_an_underscore() {
        assert!(ident_ok("_oo"));
        assert!(ident_ok("pt_0c7aa3eae81470fae5787d6aadfb6d3730c81196eba85b9b4bee0d724760"));
        assert!(ident_ok("batch_a"));
        assert!(!ident_ok(""));
        assert!(!ident_ok("Batch"));
        assert!(!ident_ok("1table"));
        assert_eq!(super::quote_ident("oo_r_devread").expect("role"), "\"oo_r_devread\"");
        assert!(super::quote_ident("Admin").is_err());
    }

    #[test]
    fn development_role_sql_is_nologin_and_mapping_select_is_quoted() {
        let sql = super::dev_role_statements("oo_r_devread", "oo_a419_dst_read").expect("sql");
        assert_eq!(
            sql[0],
            "CREATE ROLE \"oo_r_devread\" NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION"
        );
        assert_eq!(sql[1], "GRANT \"oo_r_devread\" TO \"oo_a419_dst_read\" WITH INHERIT FALSE, SET TRUE");
        assert_eq!(
            super::mapping_reader_statements("oo_a419_dst_read").expect("grant"),
            [
                "GRANT SELECT ON TABLE \"_oo\".physical_mappings TO \"oo_a419_dst_read\"",
                "GRANT EXECUTE ON FUNCTION \"_oo\".lookup_physical_mapping(text, text, bigint, text, text, text, text) TO \"oo_a419_dst_read\""
            ]
        );
        assert!(super::dev_role_statements("Admin", "reader").is_err());
        assert!(super::mapping_reader_statements("Reader").is_err());
    }
}

pub fn regular_file(path: &Path, max: u64) -> Result<(), &'static str> {
    let meta = fs::symlink_metadata(path).map_err(|_| "file")?;
    if meta.file_type().is_file() && meta.len() > 0 && meta.len() <= max {
        Ok(())
    } else {
        Err("file")
    }
}

fn read_secret(path: &Path, max: u64) -> Result<String, &'static str> {
    regular_file(path, max)?;
    let mut bytes = fs::read(path).map_err(|_| "file")?;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.is_empty() || bytes.contains(&0) {
        return Err("file");
    }
    String::from_utf8(bytes).map_err(|_| "file")
}

fn rustls_connector(ca_pem: &[u8]) -> Result<MakeRustlsConnect, &'static str> {
    let certificate = CertificateDer::from_pem_slice(ca_pem).map_err(|_| "tls")?;
    let mut roots = RootCertStore::empty();
    roots.add(certificate).map_err(|_| "tls")?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let config = ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|_| "tls")?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}
