//! `mysql-preview source-sql`: one read-only SELECT against a remote MySQL source.
//!
//! Stdin is one `oo.mysql.source-sql/v1` request. Stdout is the rows in the same
//! bytes as `mysql --batch --raw --skip-column-names`: cells separated by a tab,
//! NULL as `NULL`, one newline per row, no escaping. The password is read only
//! from its 0600 file and is never printed. Failures are closed facts.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder};
use oo_connectors::SecretRef;
use oo_connectors::mysql::values::RawCell;
use oo_connectors::mysql::{
    Access, MysqlConfig, StatementNote, connect, current_user_is, full_read_grants, inspect,
    mysql_ident, query_drop, raw_cell, scalar, tls_server_name_ok,
};
use serde_json::{Map, Value};
use tokio::time::timeout;
use zeroize::Zeroizing;

use crate::config::read_secret;
use crate::error::PreviewError;

pub const PROTOCOL: &str = "oo.mysql.source-sql/v1";
const MAX_REQUEST_BYTES: u64 = 131_072;
const MAX_SQL_BYTES: usize = 65_536;
const MAX_STATEMENT_MS: u64 = 3_600_000;
const MAX_ROWS: u64 = 10_000_000;
const MAX_OUTPUT_BYTES: u64 = 268_435_456;
const CONNECT_TIMEOUT_MS: u64 = 10_000;
/// The server enforces `statement_ms`. This grace only bounds client-side waiting.
const GRACE_MS: u64 = 10_000;

/// Words that can write, change session or server state, or reach the file system.
/// Matched case-insensitively against unquoted words only. Conservative on purpose:
/// a column named like a keyword must be quoted with backticks.
const DENIED_WORDS: &[&str] = &[
    "ALTER",
    "ANALYZE",
    "BEGIN",
    "CALL",
    "CHECK",
    "COMMIT",
    "CREATE",
    "DEALLOCATE",
    "DELETE",
    "DO",
    "DROP",
    "DUMPFILE",
    "EXECUTE",
    "FLUSH",
    "GRANT",
    "HANDLER",
    "INSERT",
    "INSTALL",
    "INTO",
    "KILL",
    "LOAD",
    "LOAD_FILE",
    "LOCK",
    "OPTIMIZE",
    "OUTFILE",
    "PREPARE",
    "PROCEDURE",
    "PURGE",
    "RELEASE",
    "RENAME",
    "REPAIR",
    "REPLACE",
    "RESET",
    "REVOKE",
    "ROLLBACK",
    "SAVEPOINT",
    "SET",
    "SHUTDOWN",
    "START",
    "TRUNCATE",
    "UNINSTALL",
    "UNLOCK",
    "UPDATE",
    "USE",
    "XA",
];

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password_file: PathBuf,
    pub ca_file: Option<PathBuf>,
    pub plaintext: bool,
    pub tls_server_name: Option<String>,
    pub statement_ms: u64,
    pub max_rows: u64,
    pub max_bytes: u64,
    pub sql: String,
}

fn config(fact: &'static str) -> PreviewError {
    PreviewError::new("config", fact)
}

pub fn parse_request(bytes: &[u8]) -> Result<Request, PreviewError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| config("json"))?;
    let root = value.as_object().ok_or_else(|| config("json"))?;
    exact_keys(
        root,
        &[
            "protocol",
            "connection",
            "statement_ms",
            "max_rows",
            "max_bytes",
            "sql",
        ],
        &[],
    )?;
    if root.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
        return Err(config("protocol"));
    }
    let connection = root
        .get("connection")
        .and_then(Value::as_object)
        .ok_or_else(|| config("connection"))?;
    exact_keys(
        connection,
        &[
            "host",
            "port",
            "database",
            "user",
            "password_file",
            "ca_file",
        ],
        &["tls_server_name", "tls", "plaintext_authorized"],
    )?;
    let host = text(connection, "host")?;
    if !host_ok(host) {
        return Err(config("host"));
    }
    let port = connection
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port != 0)
        .ok_or_else(|| config("port"))?;
    let database = text(connection, "database")?;
    let user = text(connection, "user")?;
    if !mysql_ident(database) {
        return Err(config("database"));
    }
    if !mysql_ident(user) {
        return Err(config("user"));
    }
    let tls_server_name = match connection.get("tls_server_name") {
        None | Some(Value::Null) => None,
        Some(item) => Some(
            item.as_str()
                .filter(|name| tls_server_name_ok(name))
                .ok_or_else(|| config("tls_server_name"))?
                .to_owned(),
        ),
    };
    let plaintext = match (
        connection.get("tls"),
        connection.get("plaintext_authorized"),
    ) {
        (None, None | Some(Value::Bool(false))) => false,
        (Some(Value::String(tls)), None | Some(Value::Bool(false))) if tls == "required" => false,
        (Some(Value::String(tls)), Some(Value::Bool(true))) if tls == "disabled" => true,
        _ => return Err(config("source_transport")),
    };
    let ca_file = if plaintext {
        if connection.get("ca_file") != Some(&Value::Null) || tls_server_name.is_some() {
            return Err(config("source_transport"));
        }
        None
    } else {
        Some(absolute(connection, "ca_file")?)
    };
    let sql = text(root, "sql")?;
    validate_sql(sql)?;
    Ok(Request {
        host: host.to_owned(),
        port,
        database: database.to_owned(),
        user: user.to_owned(),
        password_file: absolute(connection, "password_file")?,
        ca_file,
        plaintext,
        tls_server_name,
        statement_ms: bounded(root, "statement_ms", 1, MAX_STATEMENT_MS)?,
        max_rows: bounded(root, "max_rows", 1, MAX_ROWS)?,
        max_bytes: bounded(root, "max_bytes", 1, MAX_OUTPUT_BYTES)?,
        sql: sql.to_owned(),
    })
}

fn exact_keys(
    map: &Map<String, Value>,
    required: &[&str],
    optional: &[&str],
) -> Result<(), PreviewError> {
    let allowed = |key: &str| required.contains(&key) || optional.contains(&key);
    if map.keys().any(|key| !allowed(key)) || required.iter().any(|key| !map.contains_key(*key)) {
        return Err(config("keys"));
    }
    Ok(())
}

fn text<'a>(map: &'a Map<String, Value>, key: &'static str) -> Result<&'a str, PreviewError> {
    map.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| config(key))
}

fn bounded(
    map: &Map<String, Value>,
    key: &'static str,
    low: u64,
    high: u64,
) -> Result<u64, PreviewError> {
    map.get(key)
        .and_then(Value::as_u64)
        .filter(|value| (low..=high).contains(value))
        .ok_or_else(|| PreviewError::new("budget", key))
}

fn absolute(map: &Map<String, Value>, key: &'static str) -> Result<PathBuf, PreviewError> {
    let path = Path::new(text(map, key)?);
    let clean = path.is_absolute()
        && path
            .components()
            .all(|part| !matches!(part, Component::ParentDir));
    if clean {
        Ok(path.to_path_buf())
    } else {
        Err(config(key))
    }
}

fn host_ok(host: &str) -> bool {
    let bytes = host.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 253
        && bytes.first() != Some(&b'-')
        && bytes.last() != Some(&b'-')
        && !host.contains("..")
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'.' || *byte == b'-')
}

/// Accepts exactly one `SELECT`. Quoted text is data and is not scanned. Anything
/// that could hide a second statement, a comment, an escape or a write is refused.
pub fn validate_sql(sql: &str) -> Result<(), PreviewError> {
    if sql.is_empty() || sql.len() > MAX_SQL_BYTES {
        return Err(config("sql_size"));
    }
    if sql.bytes().any(|byte| byte == b';') {
        return Err(config("sql_statement"));
    }
    if sql.bytes().any(|byte| byte == b'\\') {
        return Err(config("sql_escape"));
    }
    if sql
        .bytes()
        .any(|byte| (byte < 0x20 && !matches!(byte, b'\t' | b'\n' | b'\r')) || byte == 0x7f)
    {
        return Err(config("sql_control"));
    }
    if !select_head(sql) {
        return Err(config("sql_statement"));
    }
    let bytes = sql.as_bytes();
    let mut word = String::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if matches!(byte, b'\'' | b'"' | b'`') {
            check_word(&mut word)?;
            let mut end = index + 1;
            loop {
                match bytes.get(end) {
                    None => return Err(config("sql_quote")),
                    Some(&found) if found == byte => {
                        if bytes.get(end + 1) == Some(&byte) {
                            end += 2;
                        } else {
                            break;
                        }
                    }
                    Some(_) => end += 1,
                }
            }
            index = end + 1;
            continue;
        }
        let next = bytes.get(index + 1).copied();
        if byte == b'#'
            || (byte == b'-' && next == Some(b'-'))
            || (byte == b'/' && next == Some(b'*'))
        {
            return Err(config("sql_comment"));
        }
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' {
            word.push(char::from(byte.to_ascii_uppercase()));
        } else {
            if word == "REPLACE" && bytes[index..].iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'(') {
                word.clear();
            }
            check_word(&mut word)?;
        }
        index += 1;
    }
    check_word(&mut word)
}

fn select_head(sql: &str) -> bool {
    let head = sql.trim_start_matches([' ', '\t', '\n', '\r']).as_bytes();
    head.len() >= 6
        && head[..6].eq_ignore_ascii_case(b"SELECT")
        && head
            .get(6)
            .is_none_or(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'$'))
}

fn check_word(word: &mut String) -> Result<(), PreviewError> {
    let denied = !word.is_empty() && DENIED_WORDS.contains(&word.as_str());
    word.clear();
    if denied {
        Err(config("sql_statement"))
    } else {
        Ok(())
    }
}

/// One row in `mysql --batch --raw --skip-column-names` form.
pub fn push_row(out: &mut Vec<u8>, cells: &[Option<Vec<u8>>]) {
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            out.push(b'\t');
        }
        match cell {
            None => out.extend_from_slice(b"NULL"),
            Some(bytes) => out.extend_from_slice(bytes),
        }
    }
    out.push(b'\n');
}

/// Reads the password from a regular file that no group or other user can read.
pub fn secret_file(path: &Path) -> Result<Zeroizing<String>, PreviewError> {
    let meta = fs::symlink_metadata(path).map_err(|_| config("password_file"))?;
    if !meta.file_type().is_file() || meta.permissions().mode() & 0o077 != 0 {
        return Err(config("password_file"));
    }
    read_secret(path).map_err(|_| config("password_file"))
}

pub async fn run_stdio() -> Result<(), PreviewError> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut input)
        .map_err(|_| config("json"))?;
    if input.len() as u64 > MAX_REQUEST_BYTES {
        return Err(config("json"));
    }
    let request = parse_request(&input)?;
    drop(input);
    let rows = execute(&request).await?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&rows)
        .and_then(|()| stdout.flush())
        .map_err(|_| PreviewError::new("io", "stdout"))
}

async fn connect_request(request: &Request, password: &str) -> Result<Conn, PreviewError> {
    if request.plaintext {
        let opts = OptsBuilder::default()
            .ip_or_hostname(request.host.clone())
            .tcp_port(request.port)
            .user(Some(request.user.clone()))
            .pass(Some(password.to_owned()))
            .db_name(Some(request.database.clone()))
            .prefer_socket(false)
            .ssl_opts(None::<mysql_async::SslOpts>);
        return timeout(Duration::from_millis(CONNECT_TIMEOUT_MS), Conn::new(opts))
            .await
            .map_err(|_| PreviewError::new("connect", "connect_timeout"))?
            .map_err(|_| PreviewError::new("connect", "source"));
    }
    let source = MysqlConfig {
        mysql_host: request.host.clone(),
        mysql_port: request.port,
        mysql_database: request.database.clone(),
        mysql_user: request.user.clone(),
        mysql_ca_file: request.ca_file.clone().ok_or_else(|| config("ca_file"))?,
        mysql_tls_server_name: request.tls_server_name.clone(),
        secret_ref: SecretRef::new("secret://mysql-source-sql")
            .map_err(|_| config("secret_ref"))?,
        connect_timeout_ms: CONNECT_TIMEOUT_MS,
    };
    connect(&source, password).await.map_err(Into::into)
}

fn plaintext_facts(
    access: &Access,
    user: &str,
    log_bin: &str,
    have_ssl: &str,
) -> Result<(), PreviewError> {
    use oo_connectors::mysql::{map, version::ServerVersion};
    ServerVersion::parse(&access.version)?;
    if map::mariadb_comment(&access.version_comment)
        || !map::session_mode_ok(&access.sql_mode)
        || !current_user_is(&access.current_user, user)
        || !matches!(log_bin, "0" | "1")
        || have_ssl != "DISABLED"
        || !access.ssl_cipher.is_empty()
    {
        return Err(PreviewError::new("privilege", "source_facts"));
    }
    Ok(())
}

async fn inspect_plaintext(
    conn: &mut Conn,
    user: &str,
    notes: &mut Vec<StatementNote>,
) -> Result<(), PreviewError> {
    let cipher: Option<(String, String)> = conn
        .query_first("SHOW STATUS LIKE 'Ssl_cipher'")
        .await
        .map_err(|_| PreviewError::new("tls", "facts"))?;
    let access = Access {
        version: scalar(conn, notes, "SELECT VERSION()").await?,
        version_comment: scalar(conn, notes, "SELECT @@version_comment").await?,
        sql_mode: scalar(conn, notes, "SELECT @@session.sql_mode").await?,
        time_zone: scalar(conn, notes, "SELECT @@session.time_zone").await?,
        ssl_cipher: cipher.ok_or_else(|| PreviewError::new("tls", "facts"))?.1,
        current_user: scalar(conn, notes, "SELECT CURRENT_USER()").await?,
    };
    let log_bin = scalar(conn, notes, "SELECT @@log_bin").await?;
    let have_ssl = scalar(conn, notes, "SELECT @@have_ssl").await?;
    plaintext_facts(&access, user, &log_bin, &have_ssl)?;
    let grants: Vec<mysql_async::Row> = conn
        .query("SHOW GRANTS FOR CURRENT_USER()")
        .await
        .map_err(|_| PreviewError::new("privilege", "grants"))?;
    let grants = grants
        .into_iter()
        .map(|row| {
            row.get(0)
                .ok_or_else(|| PreviewError::new("privilege", "grants"))
        })
        .collect::<Result<Vec<String>, _>>()?;
    full_read_grants(&grants)?;
    Ok(())
}

async fn execute(request: &Request) -> Result<Vec<u8>, PreviewError> {
    let password = secret_file(&request.password_file)?;
    let mut conn = connect_request(request, &password).await?;
    let id = conn.id();
    let result = execute_on(&mut conn, request).await;
    if result.is_err() {
        let cleanup = async {
            let mut control = connect_request(request, &password).await?;
            control
                .query_drop(format!("KILL CONNECTION {id}"))
                .await
                .map_err(|_| PreviewError::new("cancel", "kill"))?;
            control
                .disconnect()
                .await
                .map_err(|_| PreviewError::new("cancel", "disconnect"))
        };
        let _ = timeout(Duration::from_secs(5), cleanup).await;
    }
    drop(conn);
    result
}

async fn execute_on(conn: &mut Conn, request: &Request) -> Result<Vec<u8>, PreviewError> {
    let mut notes: Vec<StatementNote> = Vec::new();
    if request.plaintext {
        query_drop(conn, &mut notes, "SET SESSION sql_mode = 'STRICT_ALL_TABLES,NO_ZERO_DATE,NO_ZERO_IN_DATE,ERROR_FOR_DIVISION_BY_ZERO,NO_ENGINE_SUBSTITUTION'").await?;
        query_drop(conn, &mut notes, "SET SESSION time_zone = '+00:00'").await?;
        timeout(
            Duration::from_secs(30),
            inspect_plaintext(conn, &request.user, &mut notes),
        )
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))??;
    } else {
        inspect(conn, &request.user, &mut notes).await?;
    }
    query_drop(conn, &mut notes, "SET SESSION TRANSACTION READ ONLY").await?;
    query_drop(
        conn,
        &mut notes,
        &format!("SET SESSION max_execution_time = {}", request.statement_ms),
    )
    .await?;
    query_drop(conn, &mut notes, "START TRANSACTION READ ONLY").await?;
    let read_only = scalar(conn, &mut notes, "SELECT @@session.transaction_read_only").await?;
    if read_only != "1" {
        return Err(PreviewError::new("session", "read_only"));
    }
    let deadline = Duration::from_millis(request.statement_ms.saturating_add(GRACE_MS));
    let rows = timeout(deadline, fetch(conn, request))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))??;
    query_drop(conn, &mut notes, "ROLLBACK").await?;
    Ok(rows)
}

async fn fetch(conn: &mut Conn, request: &Request) -> Result<Vec<u8>, PreviewError> {
    let mut result = conn
        .query_iter(request.sql.as_str())
        .await
        .map_err(|_| PreviewError::new("protocol", "query"))?;
    let mut out = Vec::new();
    let mut rows = 0u64;
    while let Some(row) = result
        .next()
        .await
        .map_err(|_| PreviewError::new("protocol", "query"))?
    {
        rows += 1;
        if rows > request.max_rows {
            return Err(PreviewError::new("budget", "max_rows"));
        }
        let mut cells = Vec::with_capacity(row.len());
        for index in 0..row.len() {
            cells.push(match raw_cell(&row, index)? {
                RawCell::Null => None,
                RawCell::Bytes(bytes) => Some(bytes),
                _ => return Err(PreviewError::new("protocol", "cell")),
            });
        }
        push_row(&mut out, &cells);
        if out.len() as u64 > request.max_bytes {
            return Err(PreviewError::new("budget", "max_bytes"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Profile and typed-oracle statements in the shapes `source-onboarding` sends.
    const PROFILE_SQL: &[&str] = &[
        "SELECT @@GLOBAL.gtid_executed",
        "SELECT COLUMN_NAME,DATA_TYPE,COLUMN_TYPE,IS_NULLABLE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA='oo_cb_p11' AND TABLE_NAME='t' ORDER BY ORDINAL_POSITION",
        "SELECT JSON_OBJECT('rows',COUNT(*),'nulls',COUNT(*)-COUNT(`note`),'distinct',COUNT(DISTINCT BINARY `note`),'min',CAST(MIN(`note`) AS CHAR),'max',CAST(MAX(`note`) AS CHAR)) FROM `oo_cb_p11`.`t`",
        "SELECT JSON_OBJECT('rows',COUNT(*),'nulls',COUNT(*)-COUNT(`bin_col`),'distinct',COUNT(DISTINCT BINARY `bin_col`),'min',HEX(MIN(`bin_col`)),'max',HEX(MAX(`bin_col`))) FROM `oo_cb_p11`.`t`",
        "SELECT JSON_ARRAY(`id`,`note`) FROM `oo_cb_p11`.`t` WHERE `id` > 10 ORDER BY `id` LIMIT 4096",
        "SELECT HEX(`bin_col`) FROM `oo_cb_p11`.`t` ORDER BY `id`",
        "SELECT CAST(MIN(`id`) AS CHAR), CAST(MAX(`id`) AS CHAR) FROM `oo_cb_p11`.`t`",
        "select 1 from dual",
        "  SELECT\n  `note`\tFROM t",
    ];

    #[test]
    fn profile_and_oracle_shapes_are_accepted() {
        for sql in PROFILE_SQL {
            assert!(validate_sql(sql).is_ok(), "{sql}");
        }
    }

    #[test]
    fn replace_is_a_function_only_when_called() {
        assert!(validate_sql("SELECT REPLACE(`a`,CHAR(0),'') FROM `t`").is_ok());
        assert!(validate_sql("SELECT REPLACE (`a`,'x','') FROM `t`").is_ok());
        for sql in ["REPLACE INTO t VALUES (1)", "SELECT 1 FROM t REPLACE", "SELECT 1 FROM t WHERE REPLACE INTO x"] {
            assert!(validate_sql(sql).is_err(), "{sql:?} must be refused");
        }
    }

    #[test]
    fn non_select_and_multi_statement_are_refused() {
        for sql in [
            "DELETE FROM t",
            "INSERT INTO t VALUES (1)",
            "UPDATE t SET a = 1",
            "SET SESSION max_execution_time = 1",
            "CALL p()",
            "SELECT 1; DROP TABLE t",
            "SELECT '; DROP' FROM t",
            "SELECT 1;",
            "SELECT 1 INTO OUTFILE '/tmp/x'",
            "SELECT 1 INTO @x",
            "SELECT 1 FROM t PROCEDURE ANALYSE()",
            "SELECT * FROM t FOR UPDATE",
            "(SELECT 1)",
            "WITH x AS (SELECT 1) SELECT * FROM x",
            "",
            "   ",
            "SELECTED FROM t",
        ] {
            assert!(validate_sql(sql).is_err(), "{sql:?} must be refused");
        }
    }

    #[test]
    fn comments_escapes_and_control_bytes_are_refused() {
        for sql in [
            "SELECT 1 /* x */",
            "SELECT 1 /*! DELETE FROM t */",
            "SELECT 1 -- x",
            "SELECT 1 # x",
            "SELECT '\\'' FROM t",
            "SELECT 1\u{0}",
            "SELECT 1\u{7}",
        ] {
            assert!(validate_sql(sql).is_err(), "{sql:?} must be refused");
        }
    }

    #[test]
    fn quoted_text_is_not_code() {
        for sql in [
            "SELECT 'into', 'update' FROM t",
            "SELECT `update`, `set` FROM t",
            "SELECT 'a''b' FROM t",
            "SELECT \"delete\" FROM t",
        ] {
            assert!(validate_sql(sql).is_ok(), "{sql:?} must be accepted");
        }
        assert!(validate_sql("SELECT 'unterminated FROM t").is_err());
        assert!(validate_sql("SELECT `a FROM t").is_err());
    }

    #[test]
    fn request_shape_is_exact_and_refuses_secrets_inline() {
        let base = serde_json::json!({
            "protocol": PROTOCOL,
            "connection": {
                "host": "oo-cb-p11-mysql57", "port": 3306, "database": "oo_cb_p11", "user": "oo_reader",
                "password_file": "/secrets/reader.pw", "ca_file": "/tls/ca.pem",
                "tls_server_name": "mysql57.p11.owned.example"
            },
            "statement_ms": 60000, "max_rows": 100, "max_bytes": 1048576, "sql": "SELECT 1"
        });
        let parsed = parse_request(base.to_string().as_bytes()).expect("valid request");
        assert_eq!(parsed.port, 3306);
        assert_eq!(
            parsed.tls_server_name.as_deref(),
            Some("mysql57.p11.owned.example")
        );
        assert_eq!(parsed.password_file, PathBuf::from("/secrets/reader.pw"));
        assert_eq!(parsed.sql, "SELECT 1");

        let mut inline = base.clone();
        inline["connection"]["password"] = serde_json::json!("hunter2");
        assert!(parse_request(inline.to_string().as_bytes()).is_err());

        let mut unknown = base.clone();
        unknown["recipe"] = serde_json::json!("x");
        assert!(parse_request(unknown.to_string().as_bytes()).is_err());

        let mut version = base.clone();
        version["protocol"] = serde_json::json!("oo.mysql.source-sql/v0");
        assert!(parse_request(version.to_string().as_bytes()).is_err());

        let mut relative = base.clone();
        relative["connection"]["password_file"] = serde_json::json!("reader.pw");
        assert!(parse_request(relative.to_string().as_bytes()).is_err());

        let mut zero_budget = base.clone();
        zero_budget["statement_ms"] = serde_json::json!(0);
        assert!(parse_request(zero_budget.to_string().as_bytes()).is_err());

        let mut bad_name = base.clone();
        bad_name["connection"]["tls_server_name"] = serde_json::json!("a b/c");
        assert!(parse_request(bad_name.to_string().as_bytes()).is_err());

        let mut write = base.clone();
        write["sql"] = serde_json::json!("DELETE FROM t");
        assert!(parse_request(write.to_string().as_bytes()).is_err());
    }

    #[test]
    fn legacy_transport_is_explicit_and_paired() {
        let base = serde_json::json!({
            "protocol": PROTOCOL,
            "connection": {
                "host": "localhost", "port": 3306, "database": "sample", "user": "reader",
                "password_file": "/secrets/reader", "ca_file": null,
                "tls_server_name": null, "tls": "disabled", "plaintext_authorized": true
            },
            "statement_ms": 1000, "max_rows": 10, "max_bytes": 1024, "sql": "SELECT 1"
        });
        assert!(parse_request(base.to_string().as_bytes()).is_ok());
        let mut no_name = base.clone();
        no_name["connection"]
            .as_object_mut()
            .unwrap()
            .remove("tls_server_name");
        assert!(parse_request(no_name.to_string().as_bytes()).is_ok());
        for (key, value) in [
            ("tls", serde_json::json!("preferred")),
            ("tls", Value::Null),
            ("tls", serde_json::json!("required")),
            ("plaintext_authorized", serde_json::json!(false)),
            ("plaintext_authorized", serde_json::json!("true")),
            ("plaintext_authorized", Value::Null),
            ("ca_file", serde_json::json!("/tls/dummy.pem")),
            ("tls_server_name", serde_json::json!("mysql.example")),
        ] {
            let mut bad = base.clone();
            bad["connection"][key] = value;
            assert!(
                parse_request(bad.to_string().as_bytes()).is_err(),
                "{key}={}",
                bad["connection"][key]
            );
        }
        for key in ["tls", "plaintext_authorized", "ca_file"] {
            let mut bad = base.clone();
            bad["connection"].as_object_mut().unwrap().remove(key);
            assert!(
                parse_request(bad.to_string().as_bytes()).is_err(),
                "missing {key}"
            );
        }
        let mut tls = base.clone();
        tls["connection"].as_object_mut().unwrap().remove("tls");
        tls["connection"]
            .as_object_mut()
            .unwrap()
            .remove("plaintext_authorized");
        assert!(parse_request(tls.to_string().as_bytes()).is_err());
        tls["connection"]["ca_file"] = serde_json::json!("/tls/ca.pem");
        assert!(parse_request(tls.to_string().as_bytes()).is_ok());
        tls["connection"]["tls"] = serde_json::json!("required");
        tls["connection"]["plaintext_authorized"] = serde_json::json!(false);
        assert!(parse_request(tls.to_string().as_bytes()).is_ok());
        for sql in [
            "DELETE FROM t",
            "SELECT 1; SELECT 2",
            "SELECT 1 INTO OUTFILE '/tmp/x'",
            "SELECT 1 /*x*/",
        ] {
            let mut bad = base.clone();
            bad["sql"] = serde_json::json!(sql);
            assert!(parse_request(bad.to_string().as_bytes()).is_err());
        }
    }

    #[test]
    fn legacy_transport_refuses_unsupported_actual_facts() {
        let access = || Access {
            version: "5.7.44-log".into(),
            version_comment: "MySQL Community Server (GPL)".into(),
            sql_mode: "STRICT_TRANS_TABLES,NO_ZERO_DATE,NO_ENGINE_SUBSTITUTION".into(),
            time_zone: "+00:00".into(),
            ssl_cipher: String::new(),
            current_user: "reader@%".into(),
        };
        for log_bin in ["0", "1"] {
            assert!(plaintext_facts(&access(), "reader", log_bin, "DISABLED").is_ok());
        }
        for (log_bin, have_ssl) in [
            ("2", "DISABLED"),
            ("0", "YES"),
            ("0", "NO"),
            ("OFF", "DISABLED"),
        ] {
            assert!(plaintext_facts(&access(), "reader", log_bin, have_ssl).is_err());
        }
        for version in ["5.6.51", "9.0.1", "5.7.44-MariaDB"] {
            let mut bad = access();
            bad.version = version.into();
            assert!(plaintext_facts(&bad, "reader", "0", "DISABLED").is_err());
        }
        let mut bad = access();
        bad.version_comment = "MariaDB Server".into();
        assert!(plaintext_facts(&bad, "reader", "0", "DISABLED").is_err());
        let mut bad = access();
        bad.ssl_cipher = "TLS_AES_256_GCM_SHA384".into();
        assert!(plaintext_facts(&bad, "reader", "0", "DISABLED").is_err());
        let mut bad = access();
        bad.sql_mode = "NO_BACKSLASH_ESCAPES".into();
        assert!(plaintext_facts(&bad, "reader", "0", "DISABLED").is_err());
        assert!(plaintext_facts(&access(), "other", "0", "DISABLED").is_err());
    }

    #[test]
    fn output_is_byte_compatible_with_mysql_batch_raw() {
        let mut out = Vec::new();
        push_row(
            &mut out,
            &[
                None,
                Some(b"a\tb".to_vec()),
                Some(b"line\nbreak".to_vec()),
                Some(Vec::new()),
            ],
        );
        push_row(&mut out, &[Some(vec![0x00, 0x09, 0x0a, 0xff])]);
        assert_eq!(
            out,
            b"NULL\ta\tb\tline\nbreak\t\n\x00\x09\x0a\xff\n".to_vec()
        );

        let mut hex = Vec::new();
        push_row(
            &mut hex,
            &[Some(b"00090AFF".to_vec()), Some(b"NULL".to_vec())],
        );
        assert_eq!(hex, b"00090AFF\tNULL\n".to_vec());
    }

    #[test]
    fn password_file_must_be_a_private_regular_file() {
        let root = std::env::temp_dir().join(format!("oo-p11-secret-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        use std::os::unix::fs::{PermissionsExt, symlink};
        let private = root.join("private.pw");
        std::fs::write(&private, "s3cret-value\n").unwrap();
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(secret_file(&private).unwrap().as_str(), "s3cret-value");

        let group = root.join("group.pw");
        std::fs::write(&group, "s3cret-value").unwrap();
        std::fs::set_permissions(&group, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(secret_file(&group).is_err());

        let world = root.join("world.pw");
        std::fs::write(&world, "s3cret-value").unwrap();
        std::fs::set_permissions(&world, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(secret_file(&world).is_err());

        let link = root.join("link.pw");
        symlink(&private, &link).unwrap();
        assert!(secret_file(&link).is_err());

        let missing = root.join("missing.pw");
        assert!(secret_file(&missing).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
