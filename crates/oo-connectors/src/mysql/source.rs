//! Source-side connection and catalog only; no target or pipeline credentials.
use super::map::{MappedColumn, MysqlColumn};
use super::values::RawCell;
use super::{MysqlConfig, MysqlError, SelectedTable, StatementNote, mysql_ident, sha256_hex};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, SslOpts, Value};
use std::time::Duration;
use tokio::time::timeout;
pub async fn connect(config: &MysqlConfig, password: &str) -> Result<Conn, MysqlError> {
    if config.mysql_host.is_empty()
        || config.mysql_host.len() > 253
        || config.mysql_host.chars().any(char::is_whitespace)
        || config.mysql_port == 0
        || !mysql_ident(&config.mysql_database)
        || config.mysql_user.is_empty()
        || !(1..=30000).contains(&config.connect_timeout_ms)
    {
        return Err(MysqlError::new("config", "endpoint"));
    }
    let ca =
        std::fs::metadata(&config.mysql_ca_file).map_err(|_| MysqlError::new("tls", "ca_file"))?;
    if !ca.is_file() || ca.len() == 0 || ca.len() > 1048576 {
        return Err(MysqlError::new("tls", "ca_file"));
    }
    let ssl = tls_opts(&config.mysql_ca_file)?;
    // mysql_async 0.36 OptsBuilder has no TCP connect timeout. Conn::new is bounded below.
    let opts = OptsBuilder::default()
        .ip_or_hostname(config.mysql_host.clone())
        .tcp_port(config.mysql_port)
        .user(Some(config.mysql_user.clone()))
        .pass(Some(password.to_string()))
        .db_name(Some(config.mysql_database.clone()))
        .prefer_socket(false)
        .ssl_opts(ssl);
    let connect = timeout(
        Duration::from_millis(config.connect_timeout_ms),
        Conn::new(opts),
    )
    .await;
    match connect {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(err)) => Err(driver_error(&err.to_string(), password, "connect")),
        Err(_) => Err(MysqlError::new("connect", "connect_timeout")),
    }
}

fn tls_opts(ca: &std::path::Path) -> Result<SslOpts, MysqlError> {
    let path = ca.to_path_buf();
    Ok(SslOpts::default()
        .with_root_certs(vec![path.into()])
        .with_danger_accept_invalid_certs(false)
        .with_danger_skip_domain_validation(false)
        .with_disable_built_in_roots(true))
}

fn driver_error(text: &str, secret: &str, fact: &'static str) -> MysqlError {
    if !secret.is_empty() && text.contains(secret) {
        MysqlError::new("connect", "redacted")
    } else {
        let _ = text;
        MysqlError::new("connect", fact)
    }
}

pub async fn scalar(
    conn: &mut Conn,
    notes: &mut Vec<StatementNote>,
    sql: &str,
) -> Result<String, MysqlError> {
    note(notes, sql);
    let row: Option<Row> = timeout(Duration::from_secs(30), conn.query_first(sql))
        .await
        .map_err(|_| MysqlError::new("budget", "statement_timeout"))?
        .map_err(|_| MysqlError::new("protocol", "query"))?;
    let row = row.ok_or_else(|| MysqlError::new("protocol", "query"))?;
    row_string(&row, 0)
}

pub async fn query_drop(
    conn: &mut Conn,
    notes: &mut Vec<StatementNote>,
    sql: &str,
) -> Result<(), MysqlError> {
    note(notes, sql);
    timeout(Duration::from_secs(30), conn.query_drop(sql))
        .await
        .map_err(|_| MysqlError::new("budget", "statement_timeout"))?
        .map_err(|_| MysqlError::new("protocol", "query"))?;
    Ok(())
}

async fn show_cipher(
    conn: &mut Conn,
    notes: &mut Vec<StatementNote>,
) -> Result<String, MysqlError> {
    let sql = "SHOW STATUS LIKE 'Ssl_cipher'";
    note(notes, sql);
    let row: Option<Row> = timeout(Duration::from_secs(30), conn.query_first(sql))
        .await
        .map_err(|_| MysqlError::new("budget", "statement_timeout"))?
        .map_err(|_| MysqlError::new("tls", "ssl_cipher"))?;
    let row = row.ok_or_else(|| MysqlError::new("tls", "ssl_cipher"))?;
    row_string(&row, 1)
}

async fn show_grants(
    conn: &mut Conn,
    notes: &mut Vec<StatementNote>,
) -> Result<Vec<String>, MysqlError> {
    let sql = "SHOW GRANTS FOR CURRENT_USER()";
    note(notes, sql);
    let rows: Vec<Row> = timeout(Duration::from_secs(30), conn.query(sql))
        .await
        .map_err(|_| MysqlError::new("budget", "statement_timeout"))?
        .map_err(|_| MysqlError::new("privilege", "grants"))?;
    let mut lines = Vec::new();
    for row in rows {
        lines.push(row_string(&row, 0)?);
    }
    Ok(lines)
}

pub async fn catalog_columns(
    conn: &mut Conn,
    notes: &mut Vec<StatementNote>,
    selected: &SelectedTable,
) -> Result<Vec<MysqlColumn>, MysqlError> {
    if !mysql_ident(&selected.schema) || !mysql_ident(&selected.name) {
        return Err(MysqlError::new("ident", "ident"));
    }
    let sql = "SELECT COLUMN_NAME, ORDINAL_POSITION, DATA_TYPE, COLUMN_TYPE, IS_NULLABLE, \
               CHARACTER_MAXIMUM_LENGTH, NUMERIC_PRECISION, NUMERIC_SCALE, CHARACTER_SET_NAME, COLUMN_KEY \
               FROM information_schema.COLUMNS \
               WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION";
    note(notes, sql);
    let rows: Vec<Row> = timeout(
        Duration::from_secs(30),
        conn.exec(sql, (selected.schema.as_str(), selected.name.as_str())),
    )
    .await
    .map_err(|_| MysqlError::new("budget", "statement_timeout"))?
    .map_err(|_| MysqlError::new("catalog", "columns"))?;
    if rows.is_empty() {
        return Err(MysqlError::new("catalog", "missing_table"));
    }
    let mut columns = Vec::new();
    for row in rows {
        columns.push(MysqlColumn {
            name: row_string(&row, 0)?,
            ordinal: i32::try_from(row_i64(&row, 1)?)
                .map_err(|_| MysqlError::new("catalog", "ordinal"))?,
            data_type: row_string(&row, 2)?,
            column_type: row_string(&row, 3)?,
            nullable: row_string(&row, 4)? == "YES",
            char_len: row_opt_i64(&row, 5)?,
            precision: row_opt_i64(&row, 6)?.map(|value| i32::try_from(value).unwrap_or(i32::MAX)),
            scale: row_opt_i64(&row, 7)?.map(|value| i32::try_from(value).unwrap_or(i32::MAX)),
            charset: row_opt_string(&row, 8)?,
            column_key: row_string(&row, 9)?,
        });
        if !mysql_ident(&columns.last().expect("pushed").name) {
            return Err(MysqlError::new("ident", "ident"));
        }
    }
    Ok(columns)
}

pub fn raw_cell(row: &Row, index: usize) -> Result<RawCell, MysqlError> {
    let value: Value = row
        .get(index)
        .ok_or_else(|| MysqlError::new("protocol", "cell"))?;
    Ok(match value {
        Value::NULL => RawCell::Null,
        Value::Int(value) => RawCell::Int(value),
        Value::UInt(value) => RawCell::Uint(value),
        Value::Float(value) => RawCell::Float(value),
        Value::Double(value) => RawCell::Double(value),
        Value::Bytes(value) => RawCell::Bytes(value),
        Value::Date(year, month, day, hour, minute, second, micros) => RawCell::Date {
            year,
            month,
            day,
            hour,
            minute,
            second,
            micros,
        },
        Value::Time(negative, days, hours, minutes, seconds, micros) => RawCell::Time {
            negative,
            days,
            hours,
            minutes,
            seconds,
            micros,
        },
    })
}

fn row_string(row: &Row, index: usize) -> Result<String, MysqlError> {
    match raw_cell(row, index)? {
        RawCell::Bytes(bytes) => {
            String::from_utf8(bytes).map_err(|_| MysqlError::new("protocol", "cell"))
        }
        RawCell::Int(value) => Ok(value.to_string()),
        RawCell::Uint(value) => Ok(value.to_string()),
        RawCell::Null => Err(MysqlError::new("protocol", "cell")),
        _ => Err(MysqlError::new("protocol", "cell")),
    }
}

fn row_opt_string(row: &Row, index: usize) -> Result<Option<String>, MysqlError> {
    match raw_cell(row, index)? {
        RawCell::Null => Ok(None),
        RawCell::Bytes(bytes) => Ok(Some(
            String::from_utf8(bytes).map_err(|_| MysqlError::new("protocol", "cell"))?,
        )),
        _ => Err(MysqlError::new("protocol", "cell")),
    }
}

fn row_i64(row: &Row, index: usize) -> Result<i64, MysqlError> {
    match raw_cell(row, index)? {
        RawCell::Int(value) => Ok(value),
        RawCell::Uint(value) => {
            i64::try_from(value).map_err(|_| MysqlError::new("protocol", "cell"))
        }
        RawCell::Bytes(bytes) => {
            let text = String::from_utf8(bytes).map_err(|_| MysqlError::new("protocol", "cell"))?;
            text.parse()
                .map_err(|_| MysqlError::new("protocol", "cell"))
        }
        _ => Err(MysqlError::new("protocol", "cell")),
    }
}

fn row_opt_i64(row: &Row, index: usize) -> Result<Option<i64>, MysqlError> {
    match raw_cell(row, index)? {
        RawCell::Null => Ok(None),
        _ => row_i64(row, index).map(Some),
    }
}

pub fn current_user_is(current: &str, user: &str) -> bool {
    let account = current.split('@').next().unwrap_or("");
    !account.is_empty() && account.eq_ignore_ascii_case(user)
}

fn note(notes: &mut Vec<StatementNote>, sql: &str) {
    let verb = sql
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    notes.push(StatementNote {
        verb,
        sha256: sha256_hex(sql.as_bytes()),
    });
}

pub fn keep_requested(
    mapped: Vec<MappedColumn>,
    requested: &[String],
) -> Result<Vec<MappedColumn>, MysqlError> {
    if requested.is_empty() {
        return Ok(mapped);
    }
    let mut kept = Vec::with_capacity(requested.len());
    for name in requested {
        let Some(column) = mapped
            .iter()
            .find(|column| column.plan.name == *name || column.logical_name == *name)
        else {
            return Err(MysqlError::new("catalog", "missing_column"));
        };
        if kept
            .iter()
            .any(|item: &MappedColumn| item.plan.name == column.plan.name)
        {
            return Err(MysqlError::new("catalog", "missing_column"));
        }
        kept.push(column.clone());
    }
    if kept.iter().filter(|column| column.key).count() != 1 {
        return Err(MysqlError::new("catalog", "primary_key"));
    }
    Ok(kept)
}

/// Session facts are observed before business reads. This is the strict SELECT-only
/// access path; C1 keeps its separately audited replication preflight.
pub struct Access {
    pub version: String,
    pub version_comment: String,
    pub sql_mode: String,
    pub time_zone: String,
    pub ssl_cipher: String,
    pub current_user: String,
}
pub async fn inspect(
    conn: &mut Conn,
    user: &str,
    notes: &mut Vec<StatementNote>,
) -> Result<Access, MysqlError> {
    inspect_mode(conn, user, notes, false).await
}
pub async fn inspect_replication(
    conn: &mut Conn,
    user: &str,
    notes: &mut Vec<StatementNote>,
) -> Result<Access, MysqlError> {
    inspect_mode(conn, user, notes, true).await
}
async fn inspect_mode(
    conn: &mut Conn,
    user: &str,
    notes: &mut Vec<StatementNote>,
    replication: bool,
) -> Result<Access, MysqlError> {
    use super::map::{grant_is_read_only, mariadb_comment, session_mode_ok};
    let version = scalar(conn, notes, "SELECT VERSION()").await?;
    let version_comment = scalar(conn, notes, "SELECT @@version_comment").await?;
    if mariadb_comment(&version_comment) {
        return Err(MysqlError::new("version", "mariadb"));
    }
    super::version::ServerVersion::parse(&version)?;
    let sql_mode = scalar(conn, notes, "SELECT @@session.sql_mode").await?;
    if !session_mode_ok(&sql_mode) {
        return Err(MysqlError::new("session", "sql_mode"));
    }
    let time_zone = scalar(conn, notes, "SELECT @@session.time_zone").await?;
    let ssl_cipher = show_cipher(conn, notes).await?;
    if ssl_cipher.is_empty() {
        return Err(MysqlError::new("tls", "ssl_cipher"));
    }
    let current_user = scalar(conn, notes, "SELECT CURRENT_USER()").await?;
    if !current_user_is(&current_user, user) {
        return Err(MysqlError::new("privilege", "current_user"));
    }
    let grants = show_grants(conn, notes).await?;
    if replication {
        replication_grants_ok(&grants)?;
    } else if grants.is_empty()
        || grants.iter().any(|s| !grant_is_read_only(s))
        || !grants
            .iter()
            .any(|s| s.to_ascii_uppercase().contains("SELECT"))
    {
        return Err(MysqlError::new("privilege", "grants"));
    }
    Ok(Access {
        version,
        version_comment,
        sql_mode,
        time_zone,
        ssl_cipher,
        current_user,
    })
}

/// C1 permits its already required read-only replication privileges, never an
/// arbitrary write-capable credential or unknown privilege.
pub fn replication_grants_ok(grants: &[String]) -> Result<(), MysqlError> {
    let mut observed = std::collections::BTreeSet::new();
    for grant in grants {
        let upper = grant.to_ascii_uppercase();
        let privileges = upper
            .strip_prefix("GRANT ")
            .and_then(|s| s.split_once(" ON "))
            .map(|v| v.0)
            .ok_or_else(|| MysqlError::new("privilege", "readonly_replication"))?;
        if upper.contains("GRANT OPTION") {
            return Err(MysqlError::new("privilege", "readonly_replication"));
        }
        // SHOW GRANTS may scope SELECT to a comma-separated column list.
        // Remove only that validated identifier list before parsing privileges.
        let mut outside = String::new();
        let mut inside = false;
        let mut count = 0usize;
        for ch in privileges.chars() {
            match ch {
                '(' if !inside => {
                    inside = true;
                    count = 0;
                }
                ')' if inside && count > 0 => inside = false,
                c if inside
                    && (c.is_ascii_alphanumeric() || matches!(c, '_' | '`' | ',' | ' ')) =>
                {
                    count += usize::from(c.is_ascii_alphanumeric() || c == '_');
                }
                '(' | ')' => return Err(MysqlError::new("privilege", "readonly_replication")),
                c if inside => {
                    let _ = c;
                    return Err(MysqlError::new("privilege", "readonly_replication"));
                }
                c => outside.push(c),
            }
        }
        if inside {
            return Err(MysqlError::new("privilege", "readonly_replication"));
        }
        for name in outside.split(',').map(str::trim) {
            if !["SELECT", "USAGE", "REPLICATION SLAVE", "REPLICATION CLIENT"].contains(&name) {
                return Err(MysqlError::new("privilege", "readonly_replication"));
            }
            observed.insert(name.to_owned());
        }
    }
    if ["SELECT", "REPLICATION SLAVE", "REPLICATION CLIENT"]
        .iter()
        .any(|p| !observed.contains(*p))
    {
        return Err(MysqlError::new("privilege", "readonly_replication"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn c1_read_grants_preserve_column_scope_and_refuse_unknown_authority() {
        let read = vec![
            "GRANT REPLICATION SLAVE, REPLICATION CLIENT ON *.* TO reader".to_owned(),
            "GRANT SELECT (`id`, `updated_at`) ON approved.orders TO reader".to_owned(),
        ];
        assert!(replication_grants_ok(&read).is_ok());
        for privilege in [
            "INSERT",
            "TRUNCATE",
            "FILE",
            "EXECUTE",
            "SUPER",
            "CREATE USER",
            "SELECT (`id`) WITH GRANT OPTION",
        ] {
            let mut grants = read.clone();
            grants.push(format!("GRANT {privilege} ON *.* TO reader"));
            assert!(replication_grants_ok(&grants).is_err(), "{privilege}");
        }
    }
}
