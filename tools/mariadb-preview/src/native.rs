//! One reader connection. Value rows feed the copy encoder. Text rows are the oracle.

use std::time::Duration;

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, Value};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use tokio::time::timeout;

use crate::cell::{self, ColSpec, Compare, TableSpec};
use crate::config::{self, ApprovedObject, Config, Secret, SourceBudget, ident_ok, object_allowed};
use mariadb_probe::decide::{self, NativeColumn};

pub struct NativeRow {
    pub index: usize,
    pub values: Vec<Value>,
    pub text: Vec<Option<String>>,
}

pub struct NativeTable {
    pub spec: TableSpec,
    pub rows: Vec<NativeRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OracleColumn {
    pub name: String,
    pub physical: String,
    pub compare: String,
    pub scale: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OracleTable {
    pub logical_table: String,
    pub physical: String,
    pub namespace: String,
    pub key_physical: String,
    pub columns: Vec<OracleColumn>,
    pub rows: Vec<Vec<Option<String>>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OracleFile {
    pub tables: Vec<OracleTable>,
}

pub async fn read_fixture(config: &Config) -> Result<Vec<NativeTable>, String> {
    let user = Secret::load(&config.maria_user_file)?;
    let password = Secret::load(&config.maria_password_file)?;
    let secrets = [password.expose()];
    let opts = OptsBuilder::default()
        .ip_or_hostname(config.maria_host.clone())
        .tcp_port(config.maria_port)
        .user(Some(user.expose().to_string()))
        .pass(Some(password.expose().to_string()))
        .db_name(Some(crate::cell::SCHEMA.to_string()))
        .prefer_socket(false)
        .stmt_cache_size(0);
    let connected = timeout(Duration::from_secs(20), Conn::new(opts)).await;
    let mut conn = match connected {
        Ok(Ok(conn)) => conn,
        Ok(Err(err)) => return Err(config::redact(&err.to_string(), &secrets)),
        Err(_) => return Err("maria connect timeout".to_string()),
    };
    let mut zone: Vec<Row> = conn
        .query("SELECT @@time_zone, @@version")
        .await
        .map_err(|err| config::redact(&err.to_string(), &secrets))?;
    let zone_row = zone.pop().ok_or_else(|| "time zone missing".to_string())?;
    let zone_text = row_text(zone_row, 0)?;
    if zone_text.as_deref() != Some("+00:00") {
        let _ = conn.disconnect().await;
        return Err(format!("time_zone {zone_text:?}"));
    }
    let mut tables = Vec::new();
    for spec in cell::fixture_tables() {
        let loaded = read_table(&mut conn, &spec)
            .await
            .map_err(|err| config::redact(&err, &secrets))?;
        let expected = expected_count(&spec.name);
        if loaded.rows.len() != expected {
            let _ = conn.disconnect().await;
            return Err(format!(
                "{} row count {} expected {expected}",
                spec.name,
                loaded.rows.len()
            ));
        }
        tables.push(loaded);
    }
    let _ = conn.disconnect().await;
    Ok(tables)
}

fn quote_ident(name: &str) -> Result<String, String> {
    if !ident_ok(name) {
        return Err(format!("closed identifier {name}"));
    }
    Ok(format!("`{name}`"))
}

fn order_by(spec: &TableSpec) -> Result<String, String> {
    let keys = spec
        .columns
        .iter()
        .filter(|column| column.key)
        .collect::<Vec<_>>();
    if keys.is_empty() {
        return Err(format!("no key {}", spec.name));
    }
    Ok(keys
        .iter()
        .map(|column| format!("`{}`", column.name))
        .collect::<Vec<_>>()
        .join(", "))
}

fn expected_count(name: &str) -> usize {
    match name {
        "k_unsigned" | "k_decimal" | "k_json" => 6,
        "k_null" | "k_null_unique" => 3,
        "k_temporal" => 5,
        "k_binary" => 9,
        "k_zero_probe" => 0,
        _ => usize::MAX,
    }
}

async fn read_table(conn: &mut Conn, spec: &TableSpec) -> Result<NativeTable, String> {
    if !ident_ok(&spec.name) || spec.columns.iter().any(|column| !ident_ok(&column.name)) {
        return Err(format!("closed identifier {}", spec.name));
    }
    let order = order_by(spec)?;
    let schema = quote_ident(&spec.schema)?;
    let table = quote_ident(&spec.name)?;
    let list = spec
        .columns
        .iter()
        .map(|column| format!("`{}`", column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let value_sql = format!("SELECT {list} FROM {schema}.{table} ORDER BY {order}");
    let text_sql = format!(
        "SELECT {} FROM {schema}.{table} ORDER BY {order}",
        text_list(spec)
    );
    let value_rows: Vec<Row> = conn.query(value_sql).await.map_err(|err| err.to_string())?;
    let text_rows: Vec<Row> = conn.query(text_sql).await.map_err(|err| err.to_string())?;
    if value_rows.len() != text_rows.len() {
        return Err(format!("{} value and text counts differ", spec.name));
    }
    let mut rows = Vec::with_capacity(value_rows.len());
    for (index, (value_row, text_row)) in value_rows.into_iter().zip(text_rows).enumerate() {
        let values = take_values(value_row, spec.columns.len())?;
        let text_values = take_values(text_row, spec.columns.len())?;
        let text = text_values
            .into_iter()
            .map(value_text)
            .collect::<Result<Vec<_>, _>>()?;
        rows.push(NativeRow {
            index,
            values,
            text,
        });
    }
    Ok(NativeTable {
        spec: spec.clone(),
        rows,
    })
}

fn take_values(mut row: Row, width: usize) -> Result<Vec<Value>, String> {
    let mut values = Vec::with_capacity(width);
    for index in 0..width {
        values.push(row.take(index).ok_or_else(|| format!("column {index}"))?);
    }
    Ok(values)
}

fn row_text(row: Row, index: usize) -> Result<Option<String>, String> {
    let value = take_values(row, index + 1)?.swap_remove(index);
    value_text(value)
}

fn value_text(value: Value) -> Result<Option<String>, String> {
    match value {
        Value::NULL => Ok(None),
        Value::Bytes(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| "text utf8".to_string()),
        Value::Int(number) => Ok(Some(number.to_string())),
        Value::UInt(number) => Ok(Some(number.to_string())),
        _ => Err("text shape".to_string()),
    }
}

fn text_list(spec: &TableSpec) -> String {
    spec.columns
        .iter()
        .map(|column| {
            let name = format!("`{}`", column.name);
            let expr = match column.compare {
                Compare::Hex => format!("LOWER(HEX({name}))"),
                Compare::Date => format!("DATE_FORMAT({name}, '%Y-%m-%d')"),
                Compare::Time => format!("TIME_FORMAT({name}, '%H:%i:%s.%f')"),
                Compare::Timestamp { .. } => format!("DATE_FORMAT({name}, '%Y-%m-%d %H:%i:%s.%f')"),
                Compare::Numeric { .. } | Compare::Text | Compare::Json => {
                    format!("CAST({name} AS CHAR)")
                }
            };
            format!("CASE WHEN {name} IS NULL THEN NULL ELSE {expr} END")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn input_preimage(tables: &[NativeTable]) -> String {
    let mut out = String::new();
    for table in tables {
        out.push_str(&table.spec.schema);
        out.push('.');
        out.push_str(&table.spec.name);
        out.push('\n');
        for row in &table.rows {
            out.push_str(&row.index.to_string());
            for (column, cell) in table.spec.columns.iter().zip(&row.text) {
                out.push('\t');
                out.push_str(&column.name);
                out.push('=');
                match cell {
                    Some(text) => out.push_str(text),
                    None => out.push_str("\\N"),
                }
            }
            out.push('\n');
        }
    }
    out
}

pub fn texts_match(column: &ColSpec, native: Option<&str>, stored: Option<&str>) -> bool {
    stored_match(compare_label(column), compare_scale(column), native, stored)
}

pub fn stored_match(
    compare: &str,
    scale: Option<i32>,
    native: Option<&str>,
    stored: Option<&str>,
) -> bool {
    match (native, stored) {
        (None, None) => true,
        (Some(left), Some(right)) => match compare {
            "numeric" => numeric_eq(left, right, scale.unwrap_or(0)),
            "json" => json_eq(left, right),
            "hex" => left.eq_ignore_ascii_case(right),
            "text" | "date" | "time" | "timestamp" | "timestamptz" => left == right,
            _ => false,
        },
        _ => false,
    }
}

fn numeric_eq(left: &str, right: &str, scale: i32) -> bool {
    let Ok(left) = oo_core::ExactDecimal::parse_sql(left.trim()) else {
        return false;
    };
    let Ok(right) = oo_core::ExactDecimal::parse_sql(right.trim()) else {
        return false;
    };
    let Some(left) = cell::fit_scale(left, scale) else {
        return false;
    };
    let Some(right) = cell::fit_scale(right, scale) else {
        return false;
    };
    left.coefficient() == right.coefficient() && left.is_negative() == right.is_negative()
}

fn json_eq(left: &str, right: &str) -> bool {
    match (
        serde_json::from_str::<Json>(left),
        serde_json::from_str::<Json>(right),
    ) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

pub struct Tally {
    pub rows: u64,
    pub bytes: u64,
}

pub struct Reader {
    pub business: Conn,
    pub control: Conn,
    pub business_id: u64,
    pub control_id: u64,
    pub started: String,
}

impl Reader {
    pub async fn open(config: &Config) -> Result<Self, String> {
        let user = Secret::load(&config.maria_user_file)?;
        let password = Secret::load(&config.maria_password_file)?;
        let secrets = [password.expose()];
        let business = match open_conn(config, user.expose(), password.expose()).await {
            Ok(conn) => conn,
            Err(err) => return Err(config::redact(&err, &secrets)),
        };
        let control = match open_conn(config, user.expose(), password.expose()).await {
            Ok(conn) => conn,
            Err(err) => return Err(config::redact(&err, &secrets)),
        };
        let mut reader = Self {
            business,
            control,
            business_id: 0,
            control_id: 0,
            started: String::new(),
        };
        let (business_id, started) = match session_mark(&mut reader.business, true).await {
            Ok(mark) => mark,
            Err(err) => {
                reader.disconnect().await;
                return Err(err);
            }
        };
        let (control_id, _) = match session_mark(&mut reader.control, false).await {
            Ok(mark) => mark,
            Err(err) => {
                reader.disconnect().await;
                return Err(err);
            }
        };
        if business_id == control_id {
            reader.disconnect().await;
            return Err("control connection is the business connection".to_string());
        }
        reader.business_id = business_id;
        reader.control_id = control_id;
        reader.started = started;
        Ok(reader)
    }

    pub async fn disconnect(self) {
        let _ = self.business.disconnect().await;
        let _ = self.control.disconnect().await;
    }

    pub async fn kill_business(&mut self) -> Result<String, String> {
        kill_query(&mut self.control, self.business_id).await
    }
}

async fn kill_query(control: &mut Conn, business_id: u64) -> Result<String, String> {
    let statement = format!("KILL QUERY {business_id}");
    control
        .query_drop(statement.clone())
        .await
        .map_err(|err| err.to_string())?;
    Ok(statement)
}

async fn open_conn(config: &Config, user: &str, password: &str) -> Result<Conn, String> {
    let opts = OptsBuilder::default()
        .ip_or_hostname(config.maria_host.clone())
        .tcp_port(config.maria_port)
        .user(Some(user.to_string()))
        .pass(Some(password.to_string()))
        .prefer_socket(false)
        .stmt_cache_size(0);
    match timeout(Duration::from_secs(20), Conn::new(opts)).await {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err("maria connect timeout".to_string()),
    }
}

async fn session_mark(conn: &mut Conn, check_zone: bool) -> Result<(u64, String), String> {
    let sql = if check_zone {
        "SELECT @@time_zone, @@version, CONNECTION_ID(), DATE_FORMAT(NOW(6), '%Y-%m-%d %H:%i:%s.%f')"
    } else {
        "SELECT CONNECTION_ID(), DATE_FORMAT(NOW(6), '%Y-%m-%d %H:%i:%s.%f')"
    };
    let mut rows: Vec<Row> = conn.query(sql).await.map_err(|err| err.to_string())?;
    let row = rows.pop().ok_or_else(|| "session missing".to_string())?;
    if check_zone {
        let zone_text = row_text_at(&row, 0)?;
        if zone_text.as_deref() != Some("+00:00") {
            return Err(format!("time_zone {zone_text:?}"));
        }
        let connection_id = row_u64(&row, 2)?;
        let started = row_text_at(&row, 3)?.unwrap_or_default();
        return Ok((connection_id, started));
    }
    let connection_id = row_u64(&row, 0)?;
    let started = row_text_at(&row, 1)?.unwrap_or_default();
    Ok((connection_id, started))
}

pub async fn server_now(conn: &mut Conn) -> Result<String, String> {
    let mut rows: Vec<Row> = conn
        .query("SELECT DATE_FORMAT(NOW(6), '%Y-%m-%d %H:%i:%s.%f')")
        .await
        .map_err(|err| err.to_string())?;
    let row = rows.pop().ok_or_else(|| "now".to_string())?;
    row_text_at(&row, 0)?.ok_or_else(|| "now".to_string())
}

fn row_text_at(row: &Row, index: usize) -> Result<Option<String>, String> {
    let value: Value = row
        .clone()
        .take(index)
        .ok_or_else(|| format!("column {index}"))?;
    value_text(value)
}

fn row_u64(row: &Row, index: usize) -> Result<u64, String> {
    let value: Value = row
        .clone()
        .take(index)
        .ok_or_else(|| format!("column {index}"))?;
    match value {
        Value::Int(number) if number >= 0 => Ok(number as u64),
        Value::UInt(number) => Ok(number),
        Value::Bytes(bytes) => String::from_utf8(bytes)
            .map_err(|_| "connection id".to_string())?
            .parse()
            .map_err(|_| "connection id".to_string()),
        _ => Err("connection id".to_string()),
    }
}

pub async fn load_specs(conn: &mut Conn, config: &Config) -> Result<Vec<TableSpec>, String> {
    let selection = config
        .selection
        .as_ref()
        .ok_or_else(|| "approved_objects required".to_string())?;
    for object in &selection.requested {
        if !object_allowed(&selection.approved, object) {
            return Err(format!("unapproved: {}.{}", object.schema, object.table));
        }
        quote_ident(&object.schema)?;
        quote_ident(&object.table)?;
    }
    let mut columns = Vec::new();
    let mut checks = Vec::new();
    let mut keys = Vec::new();
    for object in &selection.requested {
        columns.extend(json_exec(conn, COLUMN_SQL, &object.schema, &object.table).await?);
        checks.extend(json_exec(conn, CHECK_SQL, &object.schema, &object.table).await?);
        keys.extend(json_exec(conn, KEY_SQL, &object.schema, &object.table).await?);
    }
    let mut specs = Vec::new();
    for object in &selection.requested {
        specs.push(spec_for(object, &columns, &checks, &keys)?);
    }
    Ok(specs)
}

const COLUMN_SQL: &str = "SELECT JSON_OBJECT( \
    'table_schema', TABLE_SCHEMA, \
    'table_name', TABLE_NAME, \
    'column_name', COLUMN_NAME, \
    'ordinal', ORDINAL_POSITION, \
    'data_type', DATA_TYPE, \
    'column_type', COLUMN_TYPE, \
    'is_nullable', IS_NULLABLE, \
    'charset', CHARACTER_SET_NAME, \
    'collation', COLLATION_NAME, \
    'column_key', COLUMN_KEY, \
    'precision', NUMERIC_PRECISION, \
    'scale', NUMERIC_SCALE, \
    'datetime_precision', DATETIME_PRECISION, \
    'chars', CHARACTER_MAXIMUM_LENGTH) \
    FROM information_schema.COLUMNS \
    WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
    ORDER BY ORDINAL_POSITION";

const CHECK_SQL: &str = "SELECT JSON_OBJECT( \
    'table_schema', CONSTRAINT_SCHEMA, \
    'table_name', TABLE_NAME, \
    'check_clause', CHECK_CLAUSE) \
    FROM information_schema.CHECK_CONSTRAINTS \
    WHERE CONSTRAINT_SCHEMA = ? AND TABLE_NAME = ?";

const KEY_SQL: &str = "SELECT JSON_OBJECT( \
    'table_schema', TABLE_SCHEMA, \
    'table_name', TABLE_NAME, \
    'index_name', INDEX_NAME, \
    'seq', SEQ_IN_INDEX, \
    'column_name', COLUMN_NAME) \
    FROM information_schema.STATISTICS \
    WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND INDEX_NAME = 'PRIMARY' \
    ORDER BY SEQ_IN_INDEX";

fn spec_for(
    object: &ApprovedObject,
    columns: &[Json],
    checks: &[Json],
    keys: &[Json],
) -> Result<TableSpec, String> {
    let mut native_columns = Vec::new();
    for column in columns {
        if jstr(column, "table_schema")? != object.schema
            || jstr(column, "table_name")? != object.table
        {
            continue;
        }
        let name = jstr(column, "column_name")?;
        if !ident_ok(&name) {
            return Err(format!("closed identifier {name}"));
        }
        let check_clause = checks
            .iter()
            .filter(|check| {
                jstr(check, "table_schema").ok().as_deref() == Some(object.schema.as_str())
                    && jstr(check, "table_name").ok().as_deref() == Some(object.table.as_str())
            })
            .filter(|check| {
                jstr(check, "check_clause")
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(&format!("`{name}`").to_ascii_lowercase())
            })
            .map(|check| jstr(check, "check_clause").unwrap_or_default())
            .collect::<Vec<_>>()
            .join(" ");
        native_columns.push(NativeColumn {
            table: object.table.clone(),
            name,
            ordinal: i32::try_from(jint(column, "ordinal")?).map_err(|_| "ordinal".to_string())?,
            data_type: jstr(column, "data_type")?,
            column_type: jstr(column, "column_type")?,
            nullable: jstr(column, "is_nullable")?.eq_ignore_ascii_case("YES"),
            charset: jopt(column, "charset")?,
            collation: jopt(column, "collation")?,
            column_key: jopt(column, "column_key")?.unwrap_or_default(),
            precision: jopt_u32(column, "precision")?,
            scale: jopt_i32(column, "scale")?,
            datetime_precision: jopt_u32(column, "datetime_precision")?,
            check_clause: if check_clause.is_empty() {
                None
            } else {
                Some(check_clause)
            },
        });
    }
    if native_columns.is_empty() {
        return Err(format!("unapproved: {}.{}", object.schema, object.table));
    }
    let mut primary = Vec::new();
    for key in keys {
        if jstr(key, "table_schema")? == object.schema && jstr(key, "table_name")? == object.table {
            primary.push(jstr(key, "column_name")?);
        }
    }
    if primary.is_empty() {
        return Err(format!(
            "unsupported_type: {}.{} key",
            object.schema, object.table
        ));
    }
    let selected = if object.columns.is_empty() {
        native_columns
    } else {
        let mut picked = Vec::new();
        for name in &object.columns {
            let Some(column) = native_columns.iter().find(|column| &column.name == name) else {
                return Err(format!(
                    "unapproved: {}.{}.{}",
                    object.schema, object.table, name
                ));
            };
            picked.push(column.clone());
        }
        for name in &primary {
            if !picked.iter().any(|column| &column.name == name) {
                return Err(format!(
                    "unapproved: {}.{}.{}",
                    object.schema, object.table, name
                ));
            }
        }
        picked
    };
    let mut spec_columns = Vec::new();
    for (index, column) in selected.iter().enumerate() {
        let plan = decide::decide(column)
            .map_err(|err| format!("unsupported_type: {} {}", err.code, err.fact))?;
        let key = primary.iter().any(|name| name == &column.name);
        let spec = cell::from_decision(
            &column.name,
            i32::try_from(index + 1).map_err(|_| "attnum".to_string())?,
            key,
            column.nullable,
            &column.column_type,
            j_chars(columns, &object.table, &column.name)?,
            &plan,
        )?;
        spec_columns.push(spec);
    }
    Ok(TableSpec {
        schema: object.schema.clone(),
        name: object.table.clone(),
        columns: spec_columns,
    })
}

fn j_chars(columns: &[Json], table: &str, name: &str) -> Result<Option<i64>, String> {
    for column in columns {
        if jstr(column, "table_name")? == table && jstr(column, "column_name")? == name {
            return jopt_i64(column, "chars");
        }
    }
    Ok(None)
}

pub struct StreamStop {
    pub error: String,
    pub killed: bool,
    pub rows_seen: u64,
    pub rows_accepted: u64,
    pub cancel: Option<String>,
}

pub async fn read_bounded(
    reader: &mut Reader,
    spec: &TableSpec,
    budget: &SourceBudget,
    tally: &mut Tally,
) -> Result<NativeTable, StreamStop> {
    if !ident_ok(&spec.schema)
        || !ident_ok(&spec.name)
        || spec.columns.iter().any(|column| !ident_ok(&column.name))
    {
        return Err(stop_plain(format!("closed identifier {}", spec.name)));
    }
    let order = match order_by(spec) {
        Ok(order) => order,
        Err(err) => return Err(stop_plain(err)),
    };
    let schema = match quote_ident(&spec.schema) {
        Ok(schema) => schema,
        Err(err) => return Err(stop_plain(err)),
    };
    let table = match quote_ident(&spec.name) {
        Ok(table) => table,
        Err(err) => return Err(stop_plain(err)),
    };
    let list = spec
        .columns
        .iter()
        .map(|column| format!("`{}`", column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let value_sql = format!("SELECT {list} FROM {schema}.{table} ORDER BY {order}");
    let deadline = Duration::from_millis(budget.deadline_ms);
    let started = std::time::Instant::now();
    let values = match stream_values(
        &mut reader.business,
        &mut reader.control,
        reader.business_id,
        &value_sql,
        spec.columns.len(),
        StreamWindow {
            budget,
            tally,
            deadline,
            started,
        },
    )
    .await
    {
        Ok(values) => values,
        Err(stop) => return Err(stop),
    };
    let text_sql = format!(
        "SELECT {} FROM {schema}.{table} ORDER BY {order}",
        text_list(spec)
    );
    let text_rows = match stream_text(
        &mut reader.business,
        &mut reader.control,
        reader.business_id,
        &text_sql,
        spec.columns.len(),
        deadline,
        started,
    )
    .await
    {
        Ok(rows) => rows,
        Err(stop) => return Err(stop),
    };
    if values.len() != text_rows.len() {
        return Err(stop_plain(format!(
            "{} value and text counts differ",
            spec.name
        )));
    }
    let mut add_bytes = 0_u64;
    let mut rows = Vec::with_capacity(values.len());
    for (index, (row_values, text)) in values.into_iter().zip(text_rows).enumerate() {
        for value in &row_values {
            add_bytes = add_bytes.saturating_add(value_bytes(value));
        }
        rows.push(NativeRow {
            index,
            values: row_values,
            text,
        });
    }
    let add_rows = rows.len() as u64;
    tally.rows = tally.rows.saturating_add(add_rows);
    tally.bytes = tally.bytes.saturating_add(add_bytes);
    Ok(NativeTable {
        spec: spec.clone(),
        rows,
    })
}

fn stop_plain(error: String) -> StreamStop {
    StreamStop {
        error,
        killed: false,
        rows_seen: 0,
        rows_accepted: 0,
        cancel: None,
    }
}

struct StreamWindow<'a> {
    budget: &'a SourceBudget,
    tally: &'a Tally,
    deadline: Duration,
    started: std::time::Instant,
}

async fn stream_values(
    business: &mut Conn,
    control: &mut Conn,
    business_id: u64,
    sql: &str,
    width: usize,
    window: StreamWindow<'_>,
) -> Result<Vec<Vec<Value>>, StreamStop> {
    let StreamWindow {
        budget,
        tally,
        deadline,
        started,
    } = window;
    let mut result = await_deadline(
        control,
        business_id,
        deadline,
        started,
        0,
        0,
        business.query_iter(sql),
    )
    .await?;
    let mut accepted = Vec::new();
    let mut pending_bytes = 0_u64;
    let mut seen = 0_u64;
    loop {
        let row = await_deadline(
            control,
            business_id,
            deadline,
            started,
            seen,
            accepted.len() as u64,
            result.next(),
        )
        .await?;
        let Some(row) = row else {
            return Ok(accepted);
        };
        seen += 1;
        let row_values = match take_values(row, width) {
            Ok(values) => values,
            Err(err) => return Err(stop_plain(err)),
        };
        let row_bytes = row_values.iter().fold(0_u64, |total, value| {
            total.saturating_add(value_bytes(value))
        });
        let next_rows = tally
            .rows
            .saturating_add(accepted.len() as u64)
            .saturating_add(1);
        let next_bytes = tally
            .bytes
            .saturating_add(pending_bytes)
            .saturating_add(row_bytes);
        if next_rows > budget.max_rows || next_bytes > budget.max_bytes {
            let kind = if next_rows > budget.max_rows {
                "row_cap"
            } else {
                "byte_cap"
            };
            let limit = if kind == "row_cap" {
                budget.max_rows
            } else {
                budget.max_bytes
            };
            let seen_now = if kind == "row_cap" {
                next_rows
            } else {
                next_bytes
            };
            let cancel = match kill_query(control, business_id).await {
                Ok(statement) => Some(statement),
                Err(err) => {
                    return Err(StreamStop {
                        error: format!("{kind}: kill failed {err}"),
                        killed: false,
                        rows_seen: seen,
                        rows_accepted: accepted.len() as u64,
                        cancel: None,
                    });
                }
            };
            let backend = confirm_killed(&mut result).await;
            return Err(StreamStop {
                error: format!("{kind}: {seen_now} would exceed {limit}"),
                killed: backend,
                rows_seen: seen,
                rows_accepted: accepted.len() as u64,
                cancel,
            });
        }
        pending_bytes = pending_bytes.saturating_add(row_bytes);
        accepted.push(row_values);
    }
}

async fn stream_text(
    business: &mut Conn,
    control: &mut Conn,
    business_id: u64,
    sql: &str,
    width: usize,
    deadline: Duration,
    started: std::time::Instant,
) -> Result<Vec<Vec<Option<String>>>, StreamStop> {
    let mut result = await_deadline(
        control,
        business_id,
        deadline,
        started,
        0,
        0,
        business.query_iter(sql),
    )
    .await?;
    let mut rows = Vec::new();
    loop {
        let row = await_deadline(
            control,
            business_id,
            deadline,
            started,
            rows.len() as u64,
            0,
            result.next(),
        )
        .await?;
        let Some(row) = row else {
            return Ok(rows);
        };
        let values = match take_values(row, width) {
            Ok(values) => values,
            Err(err) => return Err(stop_plain(err)),
        };
        let text = match values
            .into_iter()
            .map(value_text)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(text) => text,
            Err(err) => return Err(stop_plain(err)),
        };
        rows.push(text);
    }
}

async fn await_deadline<T>(
    control: &mut Conn,
    business_id: u64,
    deadline: Duration,
    started: std::time::Instant,
    seen: u64,
    accepted: u64,
    fut: impl std::future::Future<Output = Result<T, mysql_async::Error>>,
) -> Result<T, StreamStop> {
    let remaining = deadline.saturating_sub(started.elapsed());
    let mut fut = std::pin::pin!(fut);
    if remaining.is_zero() {
        return Err(finish_kill(control, business_id, seen, accepted, fut).await);
    }
    let polled = timeout(remaining, fut.as_mut()).await;
    match polled {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) if is_killed(&err) => Err(killed_stop(business_id, seen, accepted)),
        Ok(Err(err)) => Err(stop_plain(err.to_string())),
        Err(_) => Err(finish_kill(control, business_id, seen, accepted, fut).await),
    }
}

fn killed_stop(business_id: u64, seen: u64, accepted: u64) -> StreamStop {
    StreamStop {
        error: "deadline: query killed".to_string(),
        killed: true,
        rows_seen: seen,
        rows_accepted: accepted,
        cancel: Some(format!("KILL QUERY {business_id}")),
    }
}

async fn finish_kill<T>(
    control: &mut Conn,
    business_id: u64,
    seen: u64,
    accepted: u64,
    mut fut: std::pin::Pin<&mut impl std::future::Future<Output = Result<T, mysql_async::Error>>>,
) -> StreamStop {
    let cancel = match kill_query(control, business_id).await {
        Ok(statement) => Some(statement),
        Err(err) => {
            return StreamStop {
                error: format!("deadline: kill failed {err}"),
                killed: false,
                rows_seen: seen,
                rows_accepted: accepted,
                cancel: None,
            };
        }
    };
    let backend = match timeout(Duration::from_secs(5), fut.as_mut()).await {
        Ok(Err(err)) => is_killed(&err),
        _ => false,
    };
    StreamStop {
        error: if backend {
            "deadline: query killed".to_string()
        } else {
            "deadline: client timeout without backend kill".to_string()
        },
        killed: backend,
        rows_seen: seen,
        rows_accepted: accepted,
        cancel,
    }
}

async fn confirm_killed(
    result: &mut mysql_async::QueryResult<'_, '_, mysql_async::TextProtocol>,
) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        match timeout(remaining, result.next()).await {
            Ok(Err(err)) => return is_killed(&err),
            Ok(Ok(None)) => return false,
            Ok(Ok(Some(_))) => continue,
            Err(_) => return false,
        }
    }
}

fn is_killed(err: &mysql_async::Error) -> bool {
    match err {
        mysql_async::Error::Server(server) => {
            let message = server.message.to_ascii_lowercase();
            server.code == 1317
                || server.state == "70100"
                || message.contains("interrupted")
                || message.contains("killed")
        }
        other => {
            let text = other.to_string().to_ascii_lowercase();
            text.contains("interrupted") || text.contains("killed")
        }
    }
}

fn value_bytes(value: &Value) -> u64 {
    match value {
        Value::NULL => 0,
        Value::Bytes(bytes) => bytes.len() as u64,
        Value::Int(_) | Value::UInt(_) | Value::Float(_) | Value::Double(_) => 8,
        Value::Date(_, _, _, _, _, _, _) | Value::Time(_, _, _, _, _, _) => 16,
    }
}

async fn json_exec(
    conn: &mut Conn,
    sql: &str,
    schema: &str,
    table: &str,
) -> Result<Vec<Json>, String> {
    let rows: Vec<Row> = conn
        .exec(sql, (schema.to_string(), table.to_string()))
        .await
        .map_err(|err| err.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let text = row_text(row, 0)?.ok_or_else(|| "catalog json".to_string())?;
        out.push(serde_json::from_str(&text).map_err(|_| "catalog json".to_string())?);
    }
    Ok(out)
}

fn jstr(value: &Json, key: &str) -> Result<String, String> {
    match value.get(key) {
        Some(Json::String(text)) => Ok(text.clone()),
        Some(Json::Number(number)) => Ok(number.to_string()),
        Some(Json::Null) | None => Ok(String::new()),
        Some(_) => Err(format!("catalog {key}")),
    }
}

fn jopt(value: &Json, key: &str) -> Result<Option<String>, String> {
    match value.get(key) {
        Some(Json::Null) | None => Ok(None),
        Some(Json::String(text)) if text.is_empty() => Ok(None),
        Some(Json::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(format!("catalog {key}")),
    }
}

fn jint(value: &Json, key: &str) -> Result<i64, String> {
    match value.get(key) {
        Some(Json::Number(number)) => number.as_i64().ok_or_else(|| format!("catalog {key}")),
        Some(Json::String(text)) => text.parse().map_err(|_| format!("catalog {key}")),
        _ => Err(format!("catalog {key}")),
    }
}

fn jopt_i64(value: &Json, key: &str) -> Result<Option<i64>, String> {
    match value.get(key) {
        Some(Json::Null) | None => Ok(None),
        Some(_) => jint(value, key).map(Some),
    }
}

fn jopt_u32(value: &Json, key: &str) -> Result<Option<u32>, String> {
    Ok(jopt_i64(value, key)?.and_then(|number| u32::try_from(number).ok()))
}

fn jopt_i32(value: &Json, key: &str) -> Result<Option<i32>, String> {
    Ok(jopt_i64(value, key)?.and_then(|number| i32::try_from(number).ok()))
}

pub fn compare_label(column: &ColSpec) -> &'static str {
    match column.compare {
        Compare::Numeric { .. } => "numeric",
        Compare::Text => "text",
        Compare::Json => "json",
        Compare::Hex => "hex",
        Compare::Date => "date",
        Compare::Time => "time",
        Compare::Timestamp { utc: false } => "timestamp",
        Compare::Timestamp { utc: true } => "timestamptz",
    }
}

pub fn compare_scale(column: &ColSpec) -> Option<i32> {
    match column.compare {
        Compare::Numeric { scale } => Some(scale),
        _ => None,
    }
}
