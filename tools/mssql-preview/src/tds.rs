//! Readonly TDS session. A past deadline returns before a socket.
//! Cancel is a TDS attention followed by close. The reader is not granted KILL.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tiberius::{AuthMethod, Client, ColumnData, Config, EncryptionLevel, Row, ToSql};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::arrow_batch::{encode_cells, ArrowPackage, ColumnSpec};
use crate::config::{Customer, ObjectName};
use crate::fail::Fail;
use crate::policy::{command_budget, preflight, read_secret};
use crate::sql::{admit_product_sql, load_sql, CATALOG_SQL, PERMISSION_SQL, SERVER_SQL};
use crate::types::{cell_from_column, kind_for, Cell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cleanup {
    NotOpened,
    Close,
    AttentionThenClose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustPlan {
    WebpkiRoots,
    ExtraCa(String),
}

pub fn cleanup_for(opened: bool, query_started: bool, timed_out: bool) -> Cleanup {
    if !opened {
        Cleanup::NotOpened
    } else if timed_out && query_started {
        Cleanup::AttentionThenClose
    } else {
        Cleanup::Close
    }
}

pub fn trust_plan(customer: &Customer) -> Result<TrustPlan, Fail> {
    if customer.trust_server_certificate {
        return Err(Fail::new("tls_fail_open", "trust_server_certificate is refused"));
    }
    if customer.encrypt != "required" {
        return Err(Fail::new("tls_fail_closed", "encrypt must be required"));
    }
    match &customer.ca_file {
        Some(path) => Ok(TrustPlan::ExtraCa(path.clone())),
        None => Ok(TrustPlan::WebpkiRoots),
    }
}

pub fn build_config(customer: &Customer, password: &str, budget: Duration) -> Result<Config, Fail> {
    let mut config = Config::new();
    config.host(&customer.endpoint.host);
    config.port(customer.endpoint.port);
    config.database(&customer.endpoint.database);
    config.authentication(AuthMethod::sql_server(&customer.endpoint.username, password));
    config.encryption(EncryptionLevel::Required);
    config.readonly(true);
    config.application_name("oo-mssql-preview");
    config.command_timeout(Some(budget));
    config.handshake_timeout(Some(budget));
    match trust_plan(customer)? {
        TrustPlan::WebpkiRoots => config.trust_webpki_roots(),
        TrustPlan::ExtraCa(path) => config.trust_cert_ca(path),
    }
    Ok(config)
}

pub fn permission_decision(
    can_select: Option<i32>,
    can_insert: Option<i32>,
    can_update: Option<i32>,
    can_delete: Option<i32>,
    can_alter: Option<i32>,
) -> Result<(), Fail> {
    let writes = [can_insert, can_update, can_delete, can_alter];
    if can_select.is_none() || writes.iter().any(Option::is_none) {
        return Err(Fail::new(
            "permission_unobserved",
            "a permission probe returned no observation",
        ));
    }
    if writes.into_iter().any(|flag| flag.unwrap_or(0) != 0) {
        return Err(Fail::new(
            "permission_not_readonly",
            "the login is granted a write permission",
        ));
    }
    if can_select != Some(1) {
        return Err(Fail::new("select_not_granted", "SELECT is not granted on the object"));
    }
    Ok(())
}

pub(crate) type SqlClient = Client<Compat<TcpStream>>;

pub async fn run_catalog(
    customer: &Customer,
    now_ms: u64,
    requested: Option<&ObjectName>,
) -> Result<Value, Fail> {
    let ready = preflight(customer, now_ms, requested)?;
    let secret = read_secret(&ready.secret_file)?;
    let budget = command_budget(customer, now_ms)?;
    let config = build_config(customer, &secret.0, budget)?;
    let mut client = match connect(config, budget, &secret.0).await {
        Ok(client) => client,
        Err(err) => return Err(err),
    };
    let deadline = Instant::now() + budget;
    let mut query_started = false;
    let outcome = catalog_body(&mut client, customer, requested, deadline, &mut query_started, &secret.0).await;
    let mode = match &outcome {
        Err(err) if err.code == "deadline_exceeded" => cleanup_for(true, query_started, true),
        _ => cleanup_for(true, query_started, false),
    };
    shutdown(client, mode).await;
    drop(secret);
    outcome
}

pub async fn run_load(customer: &Customer, now_ms: u64, object: &ObjectName) -> Result<ArrowPackage, Fail> {
    let ready = preflight(customer, now_ms, Some(object))?;
    let secret = read_secret(&ready.secret_file)?;
    let budget = command_budget(customer, now_ms)?;
    let config = build_config(customer, &secret.0, budget)?;
    let mut client = match connect(config, budget, &secret.0).await {
        Ok(client) => client,
        Err(err) => return Err(err),
    };
    let deadline = Instant::now() + budget;
    let mut query_started = false;
    let outcome = load_body(
        &mut client,
        object,
        ready.requested_rows,
        deadline,
        &mut query_started,
        &secret.0,
    )
    .await;
    let mode = match &outcome {
        Err(err) if err.code == "deadline_exceeded" => cleanup_for(true, query_started, true),
        _ => cleanup_for(true, query_started, false),
    };
    shutdown(client, mode).await;
    drop(secret);
    outcome
}

pub(crate) async fn connect(config: Config, budget: Duration, secret: &str) -> Result<SqlClient, Fail> {
    let addr = config.get_addr();
    match timeout(budget, async move {
        let tcp = TcpStream::connect(&addr).await.map_err(|err| driver_fail(secret, err))?;
        tcp.set_nodelay(true).map_err(|err| driver_fail(secret, err))?;
        Client::connect(config, tcp.compat_write())
            .await
            .map_err(|err| driver_fail(secret, err))
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(Fail::new("deadline_exceeded", "handshake deadline elapsed")),
    }
}

pub(crate) async fn shutdown(mut client: SqlClient, mode: Cleanup) {
    if mode == Cleanup::AttentionThenClose {
        let _ = timeout(Duration::from_secs(2),client.cancel_query()).await;
    }
    let _ = timeout(Duration::from_secs(2),client.close()).await;
}

async fn catalog_body(
    client: &mut SqlClient,
    customer: &Customer,
    requested: Option<&ObjectName>,
    deadline: Instant,
    started: &mut bool,
    secret: &str,
) -> Result<Value, Fail> {
    let server_rows = query_rows(client, SERVER_SQL, &[], deadline, started, secret).await?;
    let server = server_rows
        .first()
        .ok_or_else(|| Fail::new("connect_failed", "server identity returned no row"))?;
    let login = required_string(server, "login_name")?;
    if !login.eq_ignore_ascii_case(&customer.endpoint.username) {
        return Err(Fail::new(
            "login_mismatch",
            "observed login does not match the configured username",
        ));
    }
    let objects = match requested {
        Some(object) => std::slice::from_ref(object),
        None => customer.approved_objects.as_slice(),
    };
    let mut described = Vec::new();
    for object in objects {
        described.push(describe_object(client, object, deadline, started, secret).await?);
    }
    Ok(json!({
        "protocol": "oo.mssql-preview.catalog/v1",
        "native_source": false,
        "source_acceptance": "pending",
        "gold": false,
        "tds_query_executed": true,
        "socket_opened": true,
        "product_version": required_string(server, "product_version")?,
        "product_level": required_string(server, "product_level")?,
        "edition": required_string(server, "edition")?,
        "server_collation": required_string(server, "server_collation")?,
        "database_name": required_string(server, "database_name")?,
        "observed_login": login,
        "objects": described
    }))
}

pub(crate) async fn describe_object(
    client: &mut SqlClient,
    object: &ObjectName,
    deadline: Instant,
    started: &mut bool,
    secret: &str,
) -> Result<Value, Fail> {
    let params: [&dyn ToSql; 2] = [&object.schema, &object.name];
    let rows = query_rows(client, CATALOG_SQL, &params, deadline, started, secret).await?;
    if rows.len() > 1024 {
        return Err(Fail::new("catalog_cap", "column count exceeds 1024"));
    }
    let mut columns = Vec::new();
    for row in &rows {
        columns.push(json!({
            "name": required_string(row, "column_name")?,
            "type_name": required_string(row, "type_name")?,
            "max_length": optional_i32(row, "max_length")?,
            "precision": optional_i32(row, "precision")?,
            "scale": optional_i32(row, "scale")?,
            "nullable": optional_bool(row, "is_nullable")?,
            "is_identity": optional_bool(row, "is_identity")?,
            "collation": optional_string(row, "collation_name")?
        }));
    }
    let qualified = format!("{}.{}", object.schema, object.name);
    let perm_params: [&dyn ToSql; 1] = [&qualified];
    let perms = query_rows(client, PERMISSION_SQL, &perm_params, deadline, started, secret).await?;
    let perm = perms
        .first()
        .ok_or_else(|| Fail::new("permission_unobserved", "permission probe returned no row"))?;
    permission_decision(
        optional_i32(perm, "can_select")?,
        optional_i32(perm, "can_insert")?,
        optional_i32(perm, "can_update")?,
        optional_i32(perm, "can_delete")?,
        optional_i32(perm, "can_alter")?,
    )?;
    Ok(json!({
        "schema": object.schema,
        "name": object.name,
        "reader_acceptable": true,
        "columns": columns
    }))
}

async fn load_body(
    client: &mut SqlClient,
    object: &ObjectName,
    limit: u64,
    deadline: Instant,
    started: &mut bool,
    secret: &str,
) -> Result<ArrowPackage, Fail> {
    let described = describe_object(client, object, deadline, started, secret).await?;
    let specs = specs_from_catalog(&described)?;
    let names = specs.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
    let sql = load_sql(&object.schema, &object.name, &names, limit)?;
    let rows = query_rows(client, &sql, &[], deadline, started, secret).await?;
    if rows.len() as u64 > limit {
        return Err(Fail::new("budget_exceeded", "the server returned more rows than the cap"));
    }
    let mut decoded = Vec::new();
    for row in &rows {
        let mut cells = Vec::new();
        for (index, spec) in specs.iter().enumerate() {
            let data = row
                .get_column_data(index)
                .map_err(|_| Fail::new("catalog_column", "load column is missing"))?;
            let cell = cell_from_column(&spec.sql_type_name, spec.precision, spec.scale, data)?;
            if matches!(cell, Cell::Null) && !spec.nullable {
                return Err(Fail::new("null_not_nullable", "null is not allowed in this column"));
            }
            cells.push(cell);
        }
        decoded.push(cells);
    }
    let mut package = encode_cells(&specs, &decoded)?;
    if let Some(object) = package.manifest.as_object_mut() {
        object.insert("tds_query_executed".to_string(), Value::Bool(true));
        object.insert("executed_against_sqlserver".to_string(), Value::Bool(true));
        object.insert("native_source".to_string(), Value::Bool(false));
        object.insert("source_acceptance".to_string(), Value::String("pending".to_string()));
        object.insert("may_be_truncated".to_string(), Value::Bool(decoded.len() as u64 == limit));
    }
    Ok(package)
}

pub(crate) fn specs_from_catalog(described: &Value) -> Result<Vec<ColumnSpec>, Fail> {
    let columns = described
        .get("columns")
        .and_then(Value::as_array)
        .ok_or_else(|| Fail::new("catalog_column", "catalog columns are missing"))?;
    let mut specs = Vec::new();
    for column in columns {
        let name = column
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| Fail::new("catalog_column", "column name is missing"))?
            .to_string();
        let type_name = column
            .get("type_name")
            .and_then(Value::as_str)
            .ok_or_else(|| Fail::new("catalog_column", "column type is missing"))?;
        let mut precision = narrow_u8(column.get("precision"))?;
        let mut scale = narrow_u8(column.get("scale"))?;
        match type_name.to_ascii_lowercase().as_str() {
            "money" => {
                precision = 19;
                scale = 4;
            }
            "smallmoney" => {
                precision = 10;
                scale = 4;
            }
            _ => {}
        }
        specs.push(ColumnSpec {
            name: name.clone(),
            kind: kind_for(type_name)?,
            nullable: column
                .get("nullable")
                .and_then(Value::as_bool)
                .ok_or_else(|| Fail::new("catalog_column", "nullable"))?,
            precision,
            scale,
            collation: column.get("collation").and_then(Value::as_str).map(str::to_string),
            is_identity: column.get("is_identity").and_then(Value::as_bool).unwrap_or(false),
            sql_type_name: type_name.to_string(),
            max_length: column.get("max_length").and_then(Value::as_i64).and_then(|n|i32::try_from(n).ok()),
        });
    }
    Ok(specs)
}

async fn query_rows(
    client: &mut SqlClient,
    sql: &str,
    params: &[&dyn ToSql],
    deadline: Instant,
    started: &mut bool,
    secret: &str,
) -> Result<Vec<Row>, Fail> {
    admit_product_sql(sql)?;
    let left = deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or_else(|| Fail::new("deadline_exceeded", "command deadline elapsed"))?;
    *started = true;
    match timeout(left, async {
        let stream = client
            .query(sql, params)
            .await
            .map_err(|err| driver_fail(secret, err))?;
        stream.into_first_result().await.map_err(|err| driver_fail(secret, err))
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(Fail::new("deadline_exceeded", "command deadline elapsed")),
    }
}

pub(crate) fn driver_fail(secret: &str, err: impl std::fmt::Display) -> Fail {
    let mut detail = err.to_string();
    if secret.len() >= 8 {
        detail = detail.replace(secret, "[redacted]");
    }
    if detail.len() > 300 {
        detail.truncate(300);
    }
    Fail::new("connect_failed", detail)
}

pub(crate) fn column<'a>(row: &'a Row, name: &str) -> Result<&'a ColumnData<'static>, Fail> {
    row.get_column_data(name)
        .map_err(|_| Fail::new("catalog_column", name))
}

pub(crate) fn required_string(row: &Row, name: &str) -> Result<String, Fail> {
    optional_string(row, name)?.ok_or_else(|| Fail::new("catalog_column", name))
}

pub(crate) fn optional_string(row: &Row, name: &str) -> Result<Option<String>, Fail> {
    match column(row, name)? {
        ColumnData::String(Some(value)) => Ok(Some(value.to_string())),
        ColumnData::String(None) => Ok(None),
        _ => Err(Fail::new("catalog_column", name)),
    }
}

pub(crate) fn optional_i32(row: &Row, name: &str) -> Result<Option<i32>, Fail> {
    match column(row, name)? {
        ColumnData::I32(Some(value)) => Ok(Some(*value)),
        ColumnData::I16(Some(value)) => Ok(Some(i32::from(*value))),
        ColumnData::U8(Some(value)) => Ok(Some(i32::from(*value))),
        ColumnData::I64(Some(value)) => i32::try_from(*value)
            .map(Some)
            .map_err(|_| Fail::new("catalog_column", name)),
        ColumnData::I32(None) | ColumnData::I16(None) | ColumnData::U8(None) | ColumnData::I64(None) => Ok(None),
        _ => Err(Fail::new("catalog_column", name)),
    }
}

fn optional_bool(row: &Row, name: &str) -> Result<Option<bool>, Fail> {
    match column(row, name)? {
        ColumnData::Bit(Some(value)) => Ok(Some(*value)),
        ColumnData::Bit(None) => Ok(None),
        _ => Err(Fail::new("catalog_column", name)),
    }
}

fn narrow_u8(value: Option<&Value>) -> Result<u8, Fail> {
    let Some(value) = value else {
        return Ok(0);
    };
    if value.is_null() {
        return Ok(0);
    }
    let number = value.as_u64().ok_or_else(|| Fail::new("catalog_column", "precision"))?;
    u8::try_from(number).map_err(|_| Fail::new("catalog_column", "precision"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_customer;
    use serde_json::json;

    #[test]
    fn cleanup_matches_the_attention_rule() {
        assert_eq!(cleanup_for(false, false, true), Cleanup::NotOpened);
        assert_eq!(cleanup_for(true, true, true), Cleanup::AttentionThenClose);
        assert_eq!(cleanup_for(true, true, false), Cleanup::Close);
        assert_eq!(cleanup_for(true, false, true), Cleanup::Close);
    }

    #[test]
    fn permission_probe_refuses_writes_without_sending_write_sql() {
        assert!(permission_decision(Some(1), Some(0), Some(0), Some(0), Some(0)).is_ok());
        assert_eq!(
            permission_decision(Some(1), Some(1), Some(0), Some(0), Some(0)).unwrap_err().code,
            "permission_not_readonly"
        );
        assert_eq!(
            permission_decision(Some(0), Some(0), Some(0), Some(0), Some(0)).unwrap_err().code,
            "select_not_granted"
        );
        assert_eq!(
            permission_decision(None, Some(0), Some(0), Some(0), Some(0)).unwrap_err().code,
            "permission_unobserved"
        );
    }

    #[test]
    fn config_requires_encryption_and_does_not_trust_the_certificate() {
        let customer = sample(json!({}));
        let config = build_config(&customer, "reader-secret", Duration::from_secs(5)).unwrap();
        assert_eq!(config.get_addr(), "db.example:1433");
        assert!(matches!(trust_plan(&customer).unwrap(), TrustPlan::WebpkiRoots));
        let mut open = customer.clone();
        open.trust_server_certificate = true;
        assert_eq!(trust_plan(&open).unwrap_err().code, "tls_fail_open");
    }

    #[tokio::test]
    async fn past_deadline_does_not_open_a_socket() {
        let mut customer = sample(json!({}));
        customer.connect_deadline_unix_ms = Some(1);
        customer.endpoint.host = "203.0.113.10".to_string();
        customer.secret_ref = "/tmp/oo-dl01w1-09-missing-secret".to_string();
        let started = Instant::now();
        let result = timeout(Duration::from_secs(2), run_catalog(&customer, 5, None)).await;
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(result.expect("connect was attempted").unwrap_err().code, "deadline_exceeded");
    }

    fn sample(extra: Value) -> Customer {
        let mut value = json!({
            "protocol": "oo.mssql-preview.customer/v1",
            "source_id": "sqlserver_customer",
            "secret_ref": "/tmp/oo-dl01w1-09-missing-secret",
            "endpoint": {"host": "db.example", "port": 1433, "database": "appdb", "username": "reader"},
            "encrypt": "required",
            "trust_server_certificate": false,
            "approved_objects": [{"schema": "dbo", "name": "orders"}],
            "budget": {"unit": "Rows", "hard_limit": 10, "confidence": "declared"},
            "permissions_expected": ["SELECT"]
        });
        if let (Some(map), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
            for (key, item) in extra {
                map.insert(key.clone(), item.clone());
            }
        }
        parse_customer(&serde_json::to_vec(&value).unwrap()).unwrap()
    }
}
