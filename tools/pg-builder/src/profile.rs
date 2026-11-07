//! Read-only catalog and bounded statistics. No samples, histograms, or MCV.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ClientConfig;
use serde_json::{json, Value};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::config::{Config, Postgres};
use crate::error::PgBuilderError;

const RELTUPLES_SQL: &str = "\
SELECT c.reltuples::text \
FROM pg_catalog.pg_class AS c \
JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relname = $2";

const STATS_SQL: &str = "\
SELECT a.attname::text, a.attnum::int4, s.null_frac::text, s.n_distinct::text, s.correlation::text \
FROM pg_catalog.pg_attribute AS a \
JOIN pg_catalog.pg_class AS c ON c.oid = a.attrelid \
JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
LEFT JOIN pg_catalog.pg_stats AS s \
  ON s.schemaname = n.nspname AND s.tablename = c.relname AND s.attname = a.attname \
WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
ORDER BY a.attnum";

const TABLE_COMMENT_SQL: &str = "\
SELECT pg_catalog.obj_description(c.oid, 'pg_class') \
FROM pg_catalog.pg_class AS c \
JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relname = $2";

const COLUMN_COMMENT_SQL: &str = "\
SELECT a.attname::text, pg_catalog.col_description(a.attrelid, a.attnum) \
FROM pg_catalog.pg_attribute AS a \
JOIN pg_catalog.pg_class AS c ON c.oid = a.attrelid \
JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
ORDER BY a.attnum";

pub fn catalog_request(pg: &Postgres, source_id: &str, secret_ref: &str) -> Result<Value, PgBuilderError> {
    check_source_id(source_id)?;
    check_secret_ref(secret_ref)?;
    let mut tables = Vec::new();
    for (schema, table) in &pg.tables {
        tables.push(json!({"schema": schema, "table": table}));
    }
    Ok(json!({
        "protocol": "oo.pipeline.catalog-pg/v1",
        "source": {
            "source_id": source_id,
            "host": pg.host,
            "port": pg.port,
            "database": pg.database,
            "username": pg.username,
            "secret_ref": secret_ref,
            "password_file": pg.password_file.display().to_string(),
            "ca_file": pg.ca_file.display().to_string(),
            "tables": tables,
            "budget": {
                "connect_ms": 30000,
                "statement_ms": 30000,
                "lock_ms": 30000,
                "max_rows": 10000,
                "max_bytes": 8388608
            }
        }
    }))
}

pub fn run_catalog(
    config: &Config,
    source_id: &str,
    secret_ref: &str,
    stderr_path: Option<&Path>,
) -> Result<Vec<u8>, PgBuilderError> {
    let pg = postgres(config)?;
    if pg.tables.iter().any(|(schema, _)| schema == "_oo_load") {
        return Err(PgBuilderError::new(
            "catalog.scope",
            "private loader storage is not a dataset",
        ));
    }
    if pg.tables.iter().any(|(schema, _)| schema == "oo_load_data") {
        check_source_id(source_id)?;
        check_secret_ref(secret_ref)?;
        return runtime()?.block_on(crate::publication_catalog::capture(pg, source_id));
    }
    if pg.sslcert.is_some() {
        return Err(PgBuilderError::new(
            "catalog.scope",
            "client-auth catalog requires published Agent DB views",
        ));
    }
    let request = catalog_request(pg, source_id, secret_ref)?;
    let bytes = serde_json::to_vec(&request)
        .map_err(|_| PgBuilderError::new("catalog.schema", "the catalog request could not be encoded"))?;
    let password = read_password(&pg.password_file)?;
    if !pg.catalog_bin.is_file() {
        return Err(PgBuilderError::new("catalog.bin", "the catalog binary is not a file"));
    }
    let mut child = Command::new(&pg.catalog_bin)
        .arg("catalog-pg")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
        stdin
            .write_all(&bytes)
            .map_err(|_| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    let out_handle = std::thread::spawn(move || read_capped(stdout, 8 * 1024 * 1024));
    let err_handle = std::thread::spawn(move || read_capped(stderr, 8192));
    let status = wait_child(&mut child, Duration::from_secs(60))?;
    let out = out_handle
        .join()
        .map_err(|_| PgBuilderError::new("catalog.output", "the catalog output was refused"))?
        .map_err(|_| PgBuilderError::new("catalog.output", "the catalog output was refused"))?;
    let err = err_handle
        .join()
        .map_err(|_| PgBuilderError::new("catalog.output", "the catalog output was refused"))?
        .unwrap_or_default();
    if let Some(path) = stderr_path {
        let redacted = if contains_secret(&err, password.as_bytes()) {
            b"redacted\n".to_vec()
        } else {
            err
        };
        let _ = fs::write(path, redacted);
    }
    if contains_secret(&out, password.as_bytes()) {
        return Err(PgBuilderError::new("catalog.secret", "the catalog output contained a secret"));
    }
    if !status.success() {
        return Err(PgBuilderError::new("catalog.refused", "the catalog command was refused"));
    }
    if out.is_empty() {
        return Err(PgBuilderError::new("catalog.output", "the catalog output was refused"));
    }
    Ok(out)
}

pub fn read_profile(config: &Config) -> Result<Value, PgBuilderError> {
    let runtime = runtime()?;
    runtime.block_on(profile_task(config))
}

pub fn read_comments(config: &Config) -> Result<Value, PgBuilderError> {
    let runtime = runtime()?;
    runtime.block_on(comment_task(config))
}

async fn profile_task(config: &Config) -> Result<Value, PgBuilderError> {
    let pg = postgres(config)?;
    let client = connect(pg).await?;
    let mut tables = Vec::new();
    for (schema, name) in &pg.tables {
        let schema_name = schema.as_str();
        let table_name = name.as_str();
        let rel = client
            .query(RELTUPLES_SQL, &[&schema_name, &table_name])
            .await
            .map_err(|_| PgBuilderError::new("profile.query", "the profile query was refused"))?;
        if rel.len() != 1 {
            return Err(PgBuilderError::new("profile.missing", "a configured table was not found"));
        }
        let reltuples = rel[0].get::<_, Option<String>>(0);
        let stats = client
            .query(STATS_SQL, &[&schema_name, &table_name])
            .await
            .map_err(|_| PgBuilderError::new("profile.query", "the profile query was refused"))?;
        let mut columns = Vec::new();
        for row in stats {
            columns.push(json!({
                "name": row.get::<_, String>(0),
                "attnum": row.get::<_, i32>(1),
                "null_frac": optional_text(row.get::<_, Option<String>>(2)),
                "n_distinct": optional_text(row.get::<_, Option<String>>(3)),
                "correlation": optional_text(row.get::<_, Option<String>>(4))
            }));
        }
        tables.push(json!({
            "schema": schema,
            "name": name,
            "reltuples": optional_text(reltuples),
            "columns": columns
        }));
    }
    Ok(json!({
        "protocol": "oo.pg-builder.profile/v1",
        "sample_policy": "none",
        "tables": tables
    }))
}

async fn comment_task(config: &Config) -> Result<Value, PgBuilderError> {
    let pg = postgres(config)?;
    let client = connect(pg).await?;
    let mut tables = Vec::new();
    for (schema, name) in &pg.tables {
        let schema_name = schema.as_str();
        let table_name = name.as_str();
        let table = client
            .query(TABLE_COMMENT_SQL, &[&schema_name, &table_name])
            .await
            .map_err(|_| PgBuilderError::new("comment.query", "the comment query was refused"))?;
        if table.len() != 1 {
            return Err(PgBuilderError::new("profile.missing", "a configured table was not found"));
        }
        let text = table[0].get::<_, Option<String>>(0);
        let columns_rows = client
            .query(COLUMN_COMMENT_SQL, &[&schema_name, &table_name])
            .await
            .map_err(|_| PgBuilderError::new("comment.query", "the comment query was refused"))?;
        let mut columns = Vec::new();
        for row in columns_rows {
            columns.push(json!({
                "name": row.get::<_, String>(0),
                "text": optional_text(row.get::<_, Option<String>>(1))
            }));
        }
        tables.push(json!({
            "schema": schema,
            "name": name,
            "text": optional_text(text),
            "columns": columns
        }));
    }
    Ok(json!({
        "protocol": "oo.pg-builder.comments/v1",
        "authority": "untrusted_data",
        "tables": tables
    }))
}

pub(crate) async fn connect(pg: &Postgres) -> Result<tokio_postgres::Client, PgBuilderError> {
    let password = read_password(&pg.password_file)?;
    let ca = fs::read(&pg.ca_file)
        .map_err(|_| PgBuilderError::new("profile.tls", "the caller CA could not be read"))?;
    if ca.len() > 65536 {
        return Err(PgBuilderError::new(
            "profile.tls",
            "the caller CA could not be read",
        ));
    }
    let connector = rustls_connector(&ca, pg)?;
    let mut cfg = tokio_postgres::Config::new();
    cfg.host(&pg.host);
    cfg.port(pg.port);
    cfg.dbname(&pg.database);
    cfg.user(&pg.username);
    cfg.password(password);
    cfg.ssl_mode(tokio_postgres::config::SslMode::Require);
    cfg.application_name("pg-builder");
    cfg.connect_timeout(Duration::from_secs(30));
    let (client, connection) = cfg.connect(connector).await.map_err(|_| {
        PgBuilderError::new("profile.connect", "the profile connection was refused")
    })?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .batch_execute("SET default_transaction_read_only = on; SET statement_timeout = '10s'")
        .await
        .map_err(|_| PgBuilderError::new("profile.timeout", "the statement timeout was refused"))?;
    Ok(client)
}

fn rustls_connector(ca_pem: &[u8], pg: &Postgres) -> Result<MakeRustlsConnect, PgBuilderError> {
    let mut roots = rustls::RootCertStore::empty();
    for certificate in CertificateDer::pem_slice_iter(ca_pem) {
        roots
            .add(certificate.map_err(|_| {
                PgBuilderError::new("profile.tls", "the caller CA could not be read")
            })?)
            .map_err(|_| PgBuilderError::new("profile.tls", "the caller CA could not be read"))?;
    }
    if roots.is_empty() {
        return Err(PgBuilderError::new(
            "profile.tls",
            "the caller CA could not be read",
        ));
    }
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let builder = ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|_| PgBuilderError::new("profile.tls", "the caller CA could not be read"))?
        .with_root_certificates(roots);
    let config = match (&pg.sslcert, &pg.sslkey) {
        (Some(cert), Some(key)) => {
            let cert = read_private_pem(cert)?;
            let key = read_private_pem(key)?;
            let chain = CertificateDer::pem_slice_iter(&cert)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| PgBuilderError::new("profile.tls", "invalid client certificate"))?;
            let key = PrivateKeyDer::from_pem_slice(&key)
                .map_err(|_| PgBuilderError::new("profile.tls", "invalid client key"))?;
            builder
                .with_client_auth_cert(chain, key)
                .map_err(|_| PgBuilderError::new("profile.tls", "invalid client identity"))?
        }
        (None, None) => builder.with_no_client_auth(),
        _ => {
            return Err(PgBuilderError::new(
                "profile.tls",
                "incomplete client identity",
            ));
        }
    };
    Ok(MakeRustlsConnect::new(config))
}

pub(crate) fn read_private_pem(path: &Path) -> Result<Vec<u8>, PgBuilderError> {
    let refused =
        || PgBuilderError::new("profile.tls", "TLS secret must be a private regular file");
    let meta = fs::symlink_metadata(path).map_err(|_| refused())?;
    if !meta.file_type().is_file() || meta.len() == 0 || meta.len() > 65536 {
        return Err(refused());
    }
    let file = fs::File::open(path).map_err(|_| refused())?;
    let opened = file.metadata().map_err(|_| refused())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.mode() & 0o7777 != 0o600
            || opened.nlink() != 1
            || meta.dev() != opened.dev()
            || meta.ino() != opened.ino()
        {
            return Err(refused());
        }
    }
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| refused())?;
    if bytes.is_empty() || bytes.len() > 65536 {
        return Err(refused());
    }
    Ok(bytes)
}

fn runtime() -> Result<tokio::runtime::Runtime, PgBuilderError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| PgBuilderError::new("profile.runtime", "the profile runtime could not start"))
}

fn postgres(config: &Config) -> Result<&Postgres, PgBuilderError> {
    config
        .postgres
        .as_ref()
        .ok_or_else(|| PgBuilderError::new("config.postgres", "postgres configuration is required"))
}

pub(crate) fn read_password(path: &Path) -> Result<String, PgBuilderError> {
    let meta = fs::symlink_metadata(path)
        .map_err(|_| PgBuilderError::new("profile.secret", "the password file is not a regular file"))?;
    if !meta.file_type().is_file() || meta.len() > 4096 {
        return Err(PgBuilderError::new("profile.secret", "the password file is not a regular file"));
    }
    let mut bytes = fs::read(path)
        .map_err(|_| PgBuilderError::new("profile.secret", "the password file is not a regular file"))?;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(PgBuilderError::new("profile.secret", "the password file is not a regular file"));
    }
    String::from_utf8(bytes).map_err(|_| PgBuilderError::new("profile.secret", "the password file is not a regular file"))
}

fn optional_text(value: Option<String>) -> Value {
    match value {
        Some(text) => Value::String(text),
        None => Value::Null,
    }
}

fn check_source_id(value: &str) -> Result<(), PgBuilderError> {
    let bytes = value.as_bytes();
    if (1..=63).contains(&bytes.len())
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..].iter().all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        Ok(())
    } else {
        Err(PgBuilderError::new("catalog.schema", "the source id is not allowed"))
    }
}

fn check_secret_ref(value: &str) -> Result<(), PgBuilderError> {
    if value.starts_with("secret://")
        && (10..=256).contains(&value.len())
        && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        && !value.contains('@')
    {
        Ok(())
    } else {
        Err(PgBuilderError::new("catalog.schema", "the secret ref is not allowed"))
    }
}

pub(crate) fn contains_secret(haystack: &[u8], secret: &[u8]) -> bool {
    !secret.is_empty() && haystack.windows(secret.len()).any(|window| window == secret)
}

pub(crate) fn read_capped(mut reader: impl Read, max: usize) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok(out);
        }
        if out.len().saturating_add(read) > max {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "capped"));
        }
        out.extend_from_slice(&chunk[..read]);
    }
}

pub(crate) fn wait_child(child: &mut std::process::Child, limit: Duration) -> Result<std::process::ExitStatus, PgBuilderError> {
    let started = Instant::now();
    loop {
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PgBuilderError::new("catalog.timeout", "the catalog command timed out"));
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return Err(PgBuilderError::new("catalog.spawn", "the catalog command could not start")),
        }
    }
}

#[cfg(all(test, unix))]
mod tls_tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn tls_files_require_private_regular_single_link_files() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("tls-files-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("pem");
        fs::write(&file, b"synthetic-test-content").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_private_pem(&file).is_ok());
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_private_pem(&file).is_err());
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        let link = directory.join("symlink");
        symlink(&file, &link).unwrap();
        assert!(read_private_pem(&link).is_err());
        fs::remove_file(link).unwrap();
        let link = directory.join("hardlink");
        fs::hard_link(&file, &link).unwrap();
        assert!(read_private_pem(&file).is_err());
        fs::remove_file(link).unwrap();
        fs::write(&file, vec![0u8; 65537]).unwrap();
        assert!(read_private_pem(&file).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
