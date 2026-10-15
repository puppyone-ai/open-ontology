//! DBA fixture load. The customer command cannot call this. Database names
//! are taken from the fixture file and cannot be the retained source schemas.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, SslOpts, Value};
use serde_json::{Map, Value as JsonValue, json};
use tokio::time::timeout;

use crate::config::{mysql_ident, read_ca, read_secret};
use crate::error::{PreviewError, redact};

const DENY_DATABASE: &[&str] = &[
    "logistics",
    "erp",
    "mysql",
    "information_schema",
    "performance_schema",
    "sys",
    "oo_synth",
];
const DENY_USER: &[&str] = &["root", "oo_scale", "mysql", "sys", "oo_read", "oo_write"];

struct Fixture {
    host: String,
    port: u16,
    user: String,
    password_file: PathBuf,
    ca_file: PathBuf,
    connect_timeout_ms: u64,
    database: String,
    reader: String,
    reader_password_file: PathBuf,
    tables: Vec<FixtureTable>,
}

struct FixtureTable {
    name: String,
    columns: Vec<FixtureColumn>,
    rows: Vec<Vec<JsonValue>>,
}

struct FixtureColumn {
    name: String,
    type_name: String,
    length: Option<u64>,
    scale: Option<u64>,
    key: bool,
}

pub fn database_allowed(name: &str) -> bool {
    mysql_ident(name) && !DENY_DATABASE.contains(&name)
}

pub async fn load_fixture(config_path: &Path, out_dir: &Path) -> Result<(), PreviewError> {
    let bytes = fs::read(config_path).map_err(|_| PreviewError::new("config", "path"))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| PreviewError::new("config", "json"))?;
    let fixture = parse_fixture(text)?;
    let password = read_secret(&fixture.password_file)?;
    let reader_password = read_secret(&fixture.reader_password_file)?;
    if !reader_password.bytes().all(|byte| byte.is_ascii_hexdigit()) || reader_password.len() < 32 {
        return Err(PreviewError::new("config", "password_file"));
    }
    fs::create_dir_all(out_dir).map_err(|_| PreviewError::new("config", "out"))?;
    let mut conn = connect(&fixture, password.as_str()).await?;
    exec(
        &mut conn,
        &format!(
            "CREATE DATABASE IF NOT EXISTS `{}` CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci",
            fixture.database
        ),
    )
    .await?;
    exec(&mut conn, &format!("USE `{}`", fixture.database)).await?;
    for table in &fixture.tables {
        exec(&mut conn, &format!("DROP TABLE IF EXISTS `{}`", table.name)).await?;
        exec(&mut conn, &create_sql(table)?).await?;
        insert_rows(&mut conn, table).await?;
    }
    let quoted = quote_literal(reader_password.as_str())?;
    exec(
        &mut conn,
        &format!(
            "CREATE USER IF NOT EXISTS '{}'@'%' IDENTIFIED BY {quoted}",
            fixture.reader
        ),
    )
    .await?;
    exec(
        &mut conn,
        &format!("ALTER USER '{}'@'%' IDENTIFIED BY {quoted}", fixture.reader),
    )
    .await?;
    exec(
        &mut conn,
        &format!(
            "REVOKE ALL PRIVILEGES, GRANT OPTION FROM '{}'@'%'",
            fixture.reader
        ),
    )
    .await?;
    for table in &fixture.tables {
        exec(
            &mut conn,
            &format!(
                "GRANT SELECT ON `{}`.`{}` TO '{}'@'%'",
                fixture.database, table.name, fixture.reader
            ),
        )
        .await?;
    }
    let mut counts = Vec::new();
    for table in &fixture.tables {
        let sql = format!("SELECT CAST(COUNT(*) AS CHAR) FROM `{}`", table.name);
        let row: Option<mysql_async::Row> = conn
            .query_first(sql)
            .await
            .map_err(|_| PreviewError::new("value", "count"))?;
        let row = row.ok_or_else(|| PreviewError::new("value", "count"))?;
        let value: Value = row
            .get(0)
            .ok_or_else(|| PreviewError::new("value", "count"))?;
        let count = match value {
            Value::Bytes(bytes) => {
                String::from_utf8(bytes).map_err(|_| PreviewError::new("value", "count"))?
            }
            Value::Int(number) => number.to_string(),
            Value::UInt(number) => number.to_string(),
            _ => return Err(PreviewError::new("value", "count")),
        };
        if count != table.rows.len().to_string() {
            return Err(PreviewError::new("value", "count"));
        }
        counts.push(json!({"table": table.name, "rows": table.rows.len()}));
    }
    drop(conn);
    let body = json!({
        "protocol": "oo.mysql-preview.dba-fixture/v1",
        "database": fixture.database,
        "reader": fixture.reader,
        "tables": counts,
        "customer_path": false
    });
    fs::write(
        out_dir.join("load.json"),
        serde_json::to_vec_pretty(&body).map_err(|_| PreviewError::new("config", "out"))?,
    )
    .map_err(|_| PreviewError::new("config", "out"))?;
    let _ = password;
    let _ = reader_password;
    Ok(())
}

fn create_sql(table: &FixtureTable) -> Result<String, PreviewError> {
    let mut body = String::new();
    let mut key = None;
    for column in &table.columns {
        if !body.is_empty() {
            body.push_str(", ");
        }
        body.push('`');
        body.push_str(&column.name);
        body.push_str("` ");
        body.push_str(&sql_type(column)?);
        if column.key {
            // MySQL 8.4 rejects a nullable PRIMARY KEY column (error 1171).
            body.push_str(" NOT NULL");
            key = Some(column.name.as_str());
        } else {
            body.push_str(" NULL");
        }
    }
    let key = key.ok_or_else(|| PreviewError::new("catalog", "primary_key"))?;
    Ok(format!(
        "CREATE TABLE `{}` ({body}, PRIMARY KEY (`{key}`))",
        table.name
    ))
}

fn sql_type(column: &FixtureColumn) -> Result<String, PreviewError> {
    match column.type_name.as_str() {
        "int" => Ok("int".to_string()),
        "varchar" => {
            let length = column
                .length
                .filter(|value| (1..=255).contains(value))
                .ok_or_else(|| PreviewError::new("catalog", "type"))?;
            Ok(format!("varchar({length})"))
        }
        "text" => Ok("text".to_string()),
        "decimal" => {
            let precision = column
                .length
                .filter(|value| (1..=38).contains(value))
                .ok_or_else(|| PreviewError::new("catalog", "type"))?;
            let scale = column
                .scale
                .filter(|value| *value <= precision)
                .ok_or_else(|| PreviewError::new("catalog", "type"))?;
            Ok(format!("decimal({precision},{scale})"))
        }
        "geometry" => Ok("geometry".to_string()),
        _ => Err(PreviewError::new("catalog", "type")),
    }
}

async fn insert_rows(conn: &mut Conn, table: &FixtureTable) -> Result<(), PreviewError> {
    let mut names = String::new();
    let mut marks = String::new();
    for column in &table.columns {
        if !names.is_empty() {
            names.push_str(", ");
            marks.push_str(", ");
        }
        names.push('`');
        names.push_str(&column.name);
        names.push('`');
        if column.type_name == "geometry" {
            marks.push_str("ST_GeomFromText(?)");
        } else {
            marks.push('?');
        }
    }
    let sql = format!("INSERT INTO `{}` ({names}) VALUES ({marks})", table.name);
    for row in &table.rows {
        if row.len() != table.columns.len() {
            return Err(PreviewError::new("value", "row"));
        }
        let mut params: Vec<Value> = Vec::new();
        for (column, cell) in table.columns.iter().zip(row.iter()) {
            params.push(param(column, cell)?);
        }
        conn.exec_drop(sql.as_str(), params)
            .await
            .map_err(|_| PreviewError::new("value", "insert"))?;
    }
    Ok(())
}

fn param(column: &FixtureColumn, cell: &JsonValue) -> Result<Value, PreviewError> {
    match column.type_name.as_str() {
        "int" => Ok(Value::Int(
            cell.as_i64()
                .filter(|value| *value >= 0)
                .ok_or_else(|| PreviewError::new("value", "row"))?,
        )),
        "varchar" | "text" | "geometry" => Ok(Value::Bytes(
            cell.as_str()
                .ok_or_else(|| PreviewError::new("value", "row"))?
                .as_bytes()
                .to_vec(),
        )),
        "decimal" => {
            let text = cell
                .as_str()
                .ok_or_else(|| PreviewError::new("value", "row"))?;
            if !decimal_text(text) {
                return Err(PreviewError::new("value", "row"));
            }
            Ok(Value::Bytes(text.as_bytes().to_vec()))
        }
        _ => Err(PreviewError::new("catalog", "type")),
    }
}

fn decimal_text(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    if digits.is_empty() || digits.len() > 40 || digits.starts_with('.') || digits.ends_with('.') {
        return false;
    }
    let mut dot = false;
    digits.bytes().all(|byte| {
        if byte == b'.' {
            if dot {
                return false;
            }
            dot = true;
            return true;
        }
        byte.is_ascii_digit()
    })
}

fn quote_literal(value: &str) -> Result<String, PreviewError> {
    if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PreviewError::new("config", "password_file"));
    }
    Ok(format!("'{value}'"))
}

fn parse_fixture(text: &str) -> Result<Fixture, PreviewError> {
    let value: JsonValue =
        serde_json::from_str(text).map_err(|_| PreviewError::new("config", "json"))?;
    reject_secret_keys(&value)?;
    let map = value
        .as_object()
        .ok_or_else(|| PreviewError::new("config", "json"))?;
    exact(
        map,
        &[
            "ca_file",
            "connect_timeout_ms",
            "database",
            "host",
            "password_file",
            "port",
            "reader",
            "reader_password_file",
            "tables",
            "user",
            "version",
        ],
    )?;
    if text_of(map, "version")? != "oo.mysql-preview.dba-fixture/v1" {
        return Err(PreviewError::new("config", "version"));
    }
    if text_of(map, "user")? != "root" {
        return Err(PreviewError::new("config", "user"));
    }
    let database = text_of(map, "database")?;
    if !database_allowed(database) {
        return Err(PreviewError::new("config", "database"));
    }
    let reader = text_of(map, "reader")?;
    if !mysql_ident(reader) || DENY_USER.contains(&reader) {
        return Err(PreviewError::new("config", "user"));
    }
    let tables_value = map
        .get("tables")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| PreviewError::new("config", "tables"))?;
    if tables_value.is_empty() || tables_value.len() > 4 {
        return Err(PreviewError::new("config", "tables"));
    }
    let mut tables = Vec::new();
    for item in tables_value {
        tables.push(parse_table(item)?);
    }
    Ok(Fixture {
        host: host(text_of(map, "host")?)?,
        port: map
            .get("port")
            .and_then(JsonValue::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .filter(|port| *port != 0)
            .ok_or_else(|| PreviewError::new("config", "port"))?,
        user: "root".to_string(),
        password_file: absolute(text_of(map, "password_file")?)?,
        ca_file: absolute(text_of(map, "ca_file")?)?,
        connect_timeout_ms: map
            .get("connect_timeout_ms")
            .and_then(JsonValue::as_u64)
            .filter(|value| (1..=30_000).contains(value))
            .ok_or_else(|| PreviewError::new("budget", "range"))?,
        database: database.to_string(),
        reader: reader.to_string(),
        reader_password_file: absolute(text_of(map, "reader_password_file")?)?,
        tables,
    })
}

fn parse_table(value: &JsonValue) -> Result<FixtureTable, PreviewError> {
    let map = value
        .as_object()
        .ok_or_else(|| PreviewError::new("config", "tables"))?;
    exact(map, &["columns", "name", "rows"])?;
    let name = text_of(map, "name")?;
    if !mysql_ident(name) {
        return Err(PreviewError::new("ident", "ident"));
    }
    let column_rows = map
        .get("columns")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| PreviewError::new("config", "tables"))?;
    if column_rows.is_empty() || column_rows.len() > 8 {
        return Err(PreviewError::new("config", "tables"));
    }
    let mut columns = Vec::new();
    let mut keys = 0;
    for item in column_rows {
        let column = item
            .as_object()
            .ok_or_else(|| PreviewError::new("config", "tables"))?;
        let scale = if column.contains_key("scale") {
            exact(column, &["key", "length", "name", "scale", "type"])?;
            match column.get("scale") {
                Some(JsonValue::Null) => None,
                Some(value) => Some(
                    value
                        .as_u64()
                        .ok_or_else(|| PreviewError::new("config", "tables"))?,
                ),
                None => None,
            }
        } else {
            exact(column, &["key", "length", "name", "type"])?;
            None
        };
        let column_name = text_of(column, "name")?;
        if !mysql_ident(column_name) {
            return Err(PreviewError::new("ident", "ident"));
        }
        let key = column
            .get("key")
            .and_then(JsonValue::as_bool)
            .ok_or_else(|| PreviewError::new("config", "tables"))?;
        if key {
            keys += 1;
        }
        let length = match column.get("length") {
            Some(JsonValue::Null) => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .ok_or_else(|| PreviewError::new("config", "tables"))?,
            ),
            None => None,
        };
        columns.push(FixtureColumn {
            name: column_name.to_string(),
            type_name: text_of(column, "type")?.to_string(),
            length,
            scale,
            key,
        });
    }
    if keys != 1 {
        return Err(PreviewError::new("catalog", "primary_key"));
    }
    let row_items = map
        .get("rows")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| PreviewError::new("config", "tables"))?;
    if row_items.is_empty() || row_items.len() > 100 {
        return Err(PreviewError::new("budget", "max_rows"));
    }
    let mut rows = Vec::new();
    for item in row_items {
        let row = item
            .as_array()
            .ok_or_else(|| PreviewError::new("value", "row"))?;
        rows.push(row.clone());
    }
    Ok(FixtureTable {
        name: name.to_string(),
        columns,
        rows,
    })
}

async fn connect(fixture: &Fixture, password: &str) -> Result<Conn, PreviewError> {
    let _ = read_ca(&fixture.ca_file)?;
    let ssl = SslOpts::default()
        .with_root_certs(vec![fixture.ca_file.clone().into()])
        .with_danger_accept_invalid_certs(false)
        .with_danger_skip_domain_validation(false)
        .with_disable_built_in_roots(true);
    let opts = OptsBuilder::default()
        .ip_or_hostname(fixture.host.clone())
        .tcp_port(fixture.port)
        .user(Some(fixture.user.clone()))
        .pass(Some(password.to_string()))
        .prefer_socket(false)
        .ssl_opts(ssl);
    match timeout(
        Duration::from_millis(fixture.connect_timeout_ms),
        Conn::new(opts),
    )
    .await
    {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(err)) => {
            if redact(&err.to_string(), password) {
                Err(PreviewError::new("connect", "redacted"))
            } else {
                Err(PreviewError::new("connect", "dba"))
            }
        }
        Err(_) => Err(PreviewError::new("connect", "connect_timeout")),
    }
}

async fn exec(conn: &mut Conn, sql: &str) -> Result<(), PreviewError> {
    timeout(Duration::from_secs(30), conn.query_drop(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("protocol", "query"))?;
    Ok(())
}

fn host(value: &str) -> Result<String, PreviewError> {
    if value.is_empty()
        || value.len() > 253
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
    {
        Err(PreviewError::new("config", "host"))
    } else {
        Ok(value.to_string())
    }
}

fn absolute(value: &str) -> Result<PathBuf, PreviewError> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(PreviewError::new("config", "path"))
    }
}

fn text_of<'a>(map: &'a Map<String, JsonValue>, key: &str) -> Result<&'a str, PreviewError> {
    map.get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| PreviewError::config_field(key))
}

fn exact(map: &Map<String, JsonValue>, allowed: &[&str]) -> Result<(), PreviewError> {
    if map.len() == allowed.len() && allowed.iter().all(|key| map.contains_key(*key)) {
        Ok(())
    } else {
        Err(PreviewError::new("config", "keys"))
    }
}

fn reject_secret_keys(value: &JsonValue) -> Result<(), PreviewError> {
    match value {
        JsonValue::Object(map) => {
            for (key, child) in map {
                if matches!(
                    key.as_str(),
                    "password" | "passwd" | "secret" | "token" | "private_key" | "ca_pem"
                ) {
                    return Err(PreviewError::new("config", "secret_key"));
                }
                reject_secret_keys(child)?;
            }
            Ok(())
        }
        JsonValue::Array(items) => {
            for item in items {
                reject_secret_keys(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_schemas_are_not_a_fixture_target() {
        assert!(!database_allowed("logistics"));
        assert!(!database_allowed("oo_synth"));
        assert!(!database_allowed("erp"));
        assert!(database_allowed("yard"));
    }

    #[test]
    fn primary_key_column_is_not_null() {
        let table = FixtureTable {
            name: "dock".to_string(),
            columns: vec![
                FixtureColumn {
                    name: "dock_id".to_string(),
                    type_name: "int".to_string(),
                    length: None,
                    scale: None,
                    key: true,
                },
                FixtureColumn {
                    name: "label".to_string(),
                    type_name: "varchar".to_string(),
                    length: Some(32),
                    scale: None,
                    key: false,
                },
            ],
            rows: Vec::new(),
        };
        let sql = create_sql(&table).expect("sql");
        assert!(sql.contains("`dock_id` int NOT NULL"));
        assert!(sql.contains("`label` varchar(32) NULL"));
        assert!(sql.contains("PRIMARY KEY (`dock_id`)"));
    }

    #[test]
    fn decimal_and_text_stay_exact_fixture_types() {
        let table = FixtureTable {
            name: "berth".to_string(),
            columns: vec![
                FixtureColumn {
                    name: "berth_id".to_string(),
                    type_name: "int".to_string(),
                    length: None,
                    scale: None,
                    key: true,
                },
                FixtureColumn {
                    name: "depth".to_string(),
                    type_name: "decimal".to_string(),
                    length: Some(10),
                    scale: Some(2),
                    key: false,
                },
                FixtureColumn {
                    name: "name".to_string(),
                    type_name: "text".to_string(),
                    length: None,
                    scale: None,
                    key: false,
                },
            ],
            rows: Vec::new(),
        };
        let sql = create_sql(&table).expect("sql");
        assert!(sql.contains("`depth` decimal(10,2) NULL"));
        assert!(sql.contains("`name` text NULL"));
        let cell =
            param(&table.columns[1], &JsonValue::String("18.75".to_string())).expect("decimal");
        assert!(matches!(cell, Value::Bytes(bytes) if bytes == b"18.75"));
    }
}
