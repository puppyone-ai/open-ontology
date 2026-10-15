//! Align one approved role's column SELECT grants to its visibility list.
//! Columns absent from that list are revoked. Nothing outside the list is granted.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value as JsonValue, json};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Row};
use tokio::time::timeout;

use crate::config::{mysql_ident, read_ca, read_secret};
use crate::error::PreviewError;

struct AclRequest {
    source_id: Option<String>,
    visibility_file: PathBuf,
    host: String,
    port: u16,
    database: String,
    admin_user: String,
    admin_secret_file: PathBuf,
    ca_file: PathBuf,
    connect_timeout_ms: u64,
}

struct Visibility {
    role: String,
    project: String,
    datasets: Vec<Dataset>,
}

struct Dataset {
    schema: String,
    table: String,
    columns: Vec<String>,
}

#[derive(Debug)]
pub struct ColumnPlan {
    pub grant: Vec<String>,
    pub hidden: Vec<String>,
}

pub fn plan_columns(mapped: &[String], visible: &[String]) -> Result<ColumnPlan, PreviewError> {
    if visible.is_empty() {
        return Err(PreviewError::new("config", "columns"));
    }
    let mut grant = Vec::new();
    for name in visible {
        if !mapped.iter().any(|item| item == name) {
            return Err(PreviewError::new("catalog", "mapping_absent"));
        }
        if !grant.iter().any(|item| item == name) {
            grant.push(name.clone());
        }
    }
    let mut hidden = Vec::new();
    for name in mapped {
        if !visible.iter().any(|item| item == name) {
            hidden.push(name.clone());
        }
    }
    hidden.sort();
    Ok(ColumnPlan { grant, hidden })
}

pub async fn install_column_acl(config_path: &Path, out_path: &Path) -> Result<(), PreviewError> {
    let bytes = fs::read(config_path).map_err(|_| PreviewError::new("config", "path"))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| PreviewError::new("config", "json"))?;
    let request = parse_request(text)?;
    let visibility_bytes =
        fs::read(&request.visibility_file).map_err(|_| PreviewError::new("config", "path"))?;
    let visibility_text =
        std::str::from_utf8(&visibility_bytes).map_err(|_| PreviewError::new("config", "json"))?;
    let visibility = parse_visibility(visibility_text)?;
    let password = read_secret(&request.admin_secret_file)?;
    let ca = read_ca(&request.ca_file)?;
    let mut conn = dial(&request, password.as_str(), &ca).await?;
    let body = match align(&mut conn, &request, &visibility).await {
        Ok(body) => body,
        Err(err) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut conn).await;
            return Err(err);
        }
    };
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(|_| PreviewError::new("config", "out"))?;
    }
    fs::write(
        out_path,
        serde_json::to_vec_pretty(&body).map_err(|_| PreviewError::new("config", "out"))?,
    )
    .map_err(|_| PreviewError::new("config", "out"))?;
    let _ = password;
    Ok(())
}

async fn align(
    conn: &mut sqlx::PgConnection,
    request: &AclRequest,
    visibility: &Visibility,
) -> Result<JsonValue, PreviewError> {
    sqlx::query("BEGIN")
        .execute(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "begin"))?;
    sqlx::query("SET LOCAL search_path = pg_catalog")
        .execute(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "session"))?;
    sqlx::query("SET LOCAL statement_timeout = '30s'")
        .execute(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "session"))?;
    sqlx::query("SET LOCAL lock_timeout = '2s'")
        .execute(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "session"))?;
    let current: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "admin_user"))?;
    if current != request.admin_user {
        return Err(PreviewError::new("target", "admin_user"));
    }
    let superuser: bool =
        sqlx::query_scalar("SELECT rolsuper FROM pg_catalog.pg_roles WHERE rolname = current_user")
            .fetch_one(&mut *conn)
            .await
            .map_err(|_| PreviewError::new("target", "admin_user"))?;
    if !superuser {
        return Err(PreviewError::new("privilege", "admin"));
    }
    require_reader_shape(conn, &visibility.role).await?;
    let mut datasets = Vec::new();
    for dataset in &visibility.datasets {
        datasets.push(align_one(conn, visibility, dataset, request.source_id.as_deref()).await?);
    }
    sqlx::query("COMMIT")
        .execute(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "commit"))?;
    Ok(json!({
        "protocol": "oo.mysql-preview.role-acl/v1",
        "role": visibility.role,
        "project": visibility.project,
        "datasets": datasets
    }))
}

async fn align_one(
    conn: &mut sqlx::PgConnection,
    visibility: &Visibility,
    dataset: &Dataset,
    source_id: Option<&str>,
) -> Result<JsonValue, PreviewError> {
    let rows = sqlx::query(
        "SELECT r.run_id::text, m.level, m.logical_column, m.physical_name \
         FROM _oo.physical_mappings AS m \
         JOIN _oo.full_import_run AS r \
           ON r.source_id = m.source \
          AND r.config_digest = m.config_digest \
          AND r.source_generation = m.run_generation \
         WHERE r.state = 'awaiting_publish' \
           AND r.source_id = $1 \
           AND m.logical_namespace = $2 \
           AND m.logical_table = $3 \
           AND m.level IN ('t', 'c')",
    )
    .bind(source_id.unwrap_or(&visibility.project))
    .bind(&dataset.schema)
    .bind(&dataset.table)
    .fetch_all(&mut *conn)
    .await
    .map_err(|_| PreviewError::new("catalog", "mapping"))?;
    let mut runs = BTreeSet::new();
    let mut physical_table = String::new();
    let mut mapped = Vec::new();
    let mut physical_by_logical: Vec<(String, String)> = Vec::new();
    for row in &rows {
        let run_id: String = row
            .try_get(0)
            .map_err(|_| PreviewError::new("catalog", "mapping"))?;
        let level: String = row
            .try_get(1)
            .map_err(|_| PreviewError::new("catalog", "mapping"))?;
        let logical: String = row
            .try_get(2)
            .map_err(|_| PreviewError::new("catalog", "mapping"))?;
        let physical: String = row
            .try_get(3)
            .map_err(|_| PreviewError::new("catalog", "mapping"))?;
        runs.insert(run_id);
        if level == "t" {
            physical_ok(&physical, "pt_")?;
            if !physical_table.is_empty() {
                return Err(PreviewError::new("catalog", "mapping"));
            }
            physical_table = physical;
        } else if level == "c" {
            physical_ok(&physical, "pc_")?;
            if !mysql_ident(&logical) {
                return Err(PreviewError::new("ident", "ident"));
            }
            mapped.push(logical.clone());
            physical_by_logical.push((logical, physical));
        } else {
            return Err(PreviewError::new("catalog", "mapping"));
        }
    }
    if runs.len() != 1 || physical_table.is_empty() || mapped.is_empty() {
        return Err(PreviewError::new("catalog", "mapping"));
    }
    let plan = plan_columns(&mapped, &dataset.columns)?;
    let mut revoked = Vec::new();
    for name in &plan.hidden {
        let physical = physical_of(&physical_by_logical, name)?;
        if column_select(conn, &visibility.role, &physical_table, physical).await? {
            revoked.push(physical.to_string());
        }
    }
    if !revoked.is_empty() {
        let sql = format!(
            "REVOKE SELECT ({}) ON TABLE \"_oo\".\"{}\" FROM \"{}\"",
            quote_list(&revoked)?,
            physical_table,
            visibility.role
        );
        utility(conn, sql, "revoke").await?;
    }
    let mut granted = Vec::new();
    for name in &plan.grant {
        granted.push(physical_of(&physical_by_logical, name)?.to_string());
    }
    let grant_sql = format!(
        "GRANT SELECT ({}) ON TABLE \"_oo\".\"{}\" TO \"{}\"",
        quote_list(&granted)?,
        physical_table,
        visibility.role
    );
    utility(conn, grant_sql, "grant").await?;
    for name in &plan.grant {
        let physical = physical_of(&physical_by_logical, name)?;
        if !column_select(conn, &visibility.role, &physical_table, physical).await? {
            return Err(PreviewError::new("privilege", "grant_missing"));
        }
    }
    for name in &plan.hidden {
        let physical = physical_of(&physical_by_logical, name)?;
        if column_select(conn, &visibility.role, &physical_table, physical).await? {
            return Err(PreviewError::new("privilege", "hidden_readable"));
        }
    }
    let table_grants: i64 = sqlx::query_scalar(
        "SELECT count(*)::int8 FROM information_schema.table_privileges \
         WHERE grantee = $1 AND table_schema = '_oo' AND table_name = $2 AND privilege_type = 'SELECT'",
    )
    .bind(&visibility.role)
    .bind(&physical_table)
    .fetch_one(&mut *conn)
    .await
    .map_err(|_| PreviewError::new("privilege", "table_grant"))?;
    if table_grants != 0 {
        return Err(PreviewError::new("privilege", "table_grant"));
    }
    let hidden_name = plan
        .hidden
        .first()
        .map(|name| physical_of(&physical_by_logical, name))
        .transpose()?;
    let hidden_select = if let Some(column) = hidden_name {
        deny_hidden(conn, &visibility.role, &physical_table, column).await?;
        "denied"
    } else {
        "none"
    };
    let visible_physical = physical_of(&physical_by_logical, &plan.grant[0])?;
    utility(
        conn,
        format!("SET LOCAL ROLE \"{}\"", visibility.role),
        "role",
    )
    .await?;
    let who = sqlx::query("SELECT current_user, session_user, (SELECT rolsuper FROM pg_catalog.pg_roles WHERE rolname = current_user)")
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("privilege", "role"))?;
    let current_user: String = who
        .try_get(0)
        .map_err(|_| PreviewError::new("privilege", "role"))?;
    let super_now: bool = who
        .try_get(2)
        .map_err(|_| PreviewError::new("privilege", "role"))?;
    if current_user != visibility.role || super_now {
        return Err(PreviewError::new("privilege", "role"));
    }
    let count_sql =
        format!("SELECT count(\"{visible_physical}\")::int8 FROM \"_oo\".\"{physical_table}\"");
    let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(count_sql))
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("privilege", "visible"))?;
    sqlx::query("RESET ROLE")
        .execute(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("privilege", "role"))?;
    let revoked_logical: Vec<&str> = plan
        .hidden
        .iter()
        .filter(|name| {
            revoked.iter().any(|physical| {
                physical_of(&physical_by_logical, name).ok() == Some(physical.as_str())
            })
        })
        .map(String::as_str)
        .collect();
    Ok(json!({
        "dataset": format!("{}.{}", dataset.schema, dataset.table),
        "physical_table": physical_table,
        "visible": plan.grant,
        "revoked": revoked_logical,
        "hidden_select": hidden_select,
        "visible_count": count,
        "current_user": current_user,
        "is_superuser": super_now,
        "table_select": false
    }))
}

async fn simple(conn: &mut sqlx::PgConnection, sql: &'static str) -> Result<(), PreviewError> {
    sqlx::raw_sql(sql)
        .execute(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("target", "savepoint"))?;
    Ok(())
}

async fn utility(
    conn: &mut sqlx::PgConnection,
    sql: String,
    fact: &'static str,
) -> Result<(), PreviewError> {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *conn)
        .await
        .map_err(|_| PreviewError::new("privilege", fact))?;
    Ok(())
}

async fn deny_hidden(
    conn: &mut sqlx::PgConnection,
    role: &str,
    table: &str,
    column: &str,
) -> Result<(), PreviewError> {
    // A separate statement after SET ROLE. A privilege error aborts the
    // transaction until ROLLBACK TO SAVEPOINT, so the grant work before the
    // savepoint can still commit when the hidden column is denied.
    simple(conn, "SAVEPOINT hidden_probe").await?;
    utility(conn, format!("SET LOCAL ROLE \"{role}\""), "role").await?;
    let probe = format!("SELECT \"{column}\" FROM \"_oo\".\"{table}\" LIMIT 1");
    let read = sqlx::query(sqlx::AssertSqlSafe(probe))
        .fetch_optional(&mut *conn)
        .await;
    simple(conn, "ROLLBACK TO SAVEPOINT hidden_probe").await?;
    simple(conn, "RELEASE SAVEPOINT hidden_probe").await?;
    match read {
        Ok(_) => Err(PreviewError::new("privilege", "hidden_readable")),
        Err(err)
            if err.as_database_error().and_then(|db| db.code()).as_deref() == Some("42501") =>
        {
            Ok(())
        }
        Err(_) => Err(PreviewError::new("privilege", "hidden_probe")),
    }
}

async fn column_select(
    conn: &mut sqlx::PgConnection,
    role: &str,
    table: &str,
    column: &str,
) -> Result<bool, PreviewError> {
    let value: Option<bool> = sqlx::query_scalar(
        "SELECT pg_catalog.has_column_privilege($1::name, relation.oid, attribute.attnum, 'SELECT') \
         FROM pg_catalog.pg_class AS relation \
         JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
         JOIN pg_catalog.pg_attribute AS attribute ON attribute.attrelid = relation.oid \
         WHERE namespace.nspname = '_oo' AND relation.relname = $2 AND attribute.attname = $3 \
           AND attribute.attnum > 0 AND NOT attribute.attisdropped",
    )
    .bind(role)
    .bind(table)
    .bind(column)
    .fetch_one(&mut *conn)
    .await
    .map_err(|_| PreviewError::new("privilege", "column"))?;
    Ok(value == Some(true))
}

async fn require_reader_shape(
    conn: &mut sqlx::PgConnection,
    role: &str,
) -> Result<(), PreviewError> {
    if !role_ok(role) {
        return Err(PreviewError::new("config", "role"));
    }
    let row = sqlx::query(
        "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, rolbypassrls, rolreplication \
         FROM pg_catalog.pg_roles WHERE rolname = $1",
    )
    .bind(role)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| PreviewError::new("target", "role"))?;
    let Some(row) = row else {
        return Err(PreviewError::new("target", "role_missing"));
    };
    let flags = [
        row.try_get::<bool, _>(0)
            .map_err(|_| PreviewError::new("target", "role"))?,
        row.try_get::<bool, _>(1)
            .map_err(|_| PreviewError::new("target", "role"))?,
        row.try_get::<bool, _>(2)
            .map_err(|_| PreviewError::new("target", "role"))?,
        row.try_get::<bool, _>(3)
            .map_err(|_| PreviewError::new("target", "role"))?,
        row.try_get::<bool, _>(4)
            .map_err(|_| PreviewError::new("target", "role"))?,
        row.try_get::<bool, _>(5)
            .map_err(|_| PreviewError::new("target", "role"))?,
        row.try_get::<bool, _>(6)
            .map_err(|_| PreviewError::new("target", "role"))?,
    ];
    if flags.iter().any(|flag| *flag) {
        Err(PreviewError::new("privilege", "role"))
    } else {
        Ok(())
    }
}

fn physical_of<'a>(pairs: &'a [(String, String)], logical: &str) -> Result<&'a str, PreviewError> {
    pairs
        .iter()
        .find(|(name, _)| name == logical)
        .map(|(_, physical)| physical.as_str())
        .ok_or_else(|| PreviewError::new("catalog", "mapping_absent"))
}

fn physical_ok(name: &str, prefix: &str) -> Result<(), PreviewError> {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    if rest.len() == 60
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(PreviewError::new("catalog", "physical"))
    }
}

fn quote_list(names: &[String]) -> Result<String, PreviewError> {
    let mut list = String::new();
    for name in names {
        physical_ok(name, "pc_")?;
        if !list.is_empty() {
            list.push_str(", ");
        }
        list.push('"');
        list.push_str(name);
        list.push('"');
    }
    if list.is_empty() {
        return Err(PreviewError::new("catalog", "columns"));
    }
    Ok(list)
}

fn role_ok(role: &str) -> bool {
    let Some(rest) = role.strip_prefix("oo_r_") else {
        return false;
    };
    let bytes = rest.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 40
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn parse_request(text: &str) -> Result<AclRequest, PreviewError> {
    let value: JsonValue =
        serde_json::from_str(text).map_err(|_| PreviewError::new("config", "json"))?;
    reject_secret_keys(&value)?;
    let map = value
        .as_object()
        .ok_or_else(|| PreviewError::new("config", "json"))?;
    if map.contains_key("source_id") {
        exact(map, &["protocol", "target", "visibility_file", "source_id"])?;
    } else {
        exact(map, &["protocol", "target", "visibility_file"])?;
    }
    if text_of(map, "protocol")? != "oo.mysql-preview.role-acl/v1" {
        return Err(PreviewError::new("config", "version"));
    }
    let target = map
        .get("target")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| PreviewError::new("config", "target"))?;
    exact(
        target,
        &[
            "admin_secret_file",
            "admin_user",
            "ca_file",
            "connect_timeout_ms",
            "database",
            "host",
            "port",
        ],
    )?;
    Ok(AclRequest {
        source_id: map
            .get("source_id")
            .map(|value| {
                value
                    .as_str()
                    .filter(|name| mysql_ident(name))
                    .map(str::to_owned)
                    .ok_or_else(|| PreviewError::new("ident", "source_id"))
            })
            .transpose()?,
        visibility_file: absolute(text_of(map, "visibility_file")?)?,
        host: host(text_of(target, "host")?)?,
        port: target
            .get("port")
            .and_then(JsonValue::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .filter(|port| *port != 0)
            .ok_or_else(|| PreviewError::new("config", "port"))?,
        database: {
            let name = text_of(target, "database")?;
            if mysql_ident(name) {
                name.to_string()
            } else {
                return Err(PreviewError::new("ident", "ident"));
            }
        },
        admin_user: {
            let name = text_of(target, "admin_user")?;
            if mysql_ident(name) {
                name.to_string()
            } else {
                return Err(PreviewError::new("ident", "ident"));
            }
        },
        admin_secret_file: absolute(text_of(target, "admin_secret_file")?)?,
        ca_file: absolute(text_of(target, "ca_file")?)?,
        connect_timeout_ms: target
            .get("connect_timeout_ms")
            .and_then(JsonValue::as_u64)
            .filter(|value| (1..=30_000).contains(value))
            .ok_or_else(|| PreviewError::new("budget", "range"))?,
    })
}

fn parse_visibility(text: &str) -> Result<Visibility, PreviewError> {
    let value: JsonValue =
        serde_json::from_str(text).map_err(|_| PreviewError::new("config", "json"))?;
    let map = value
        .as_object()
        .ok_or_else(|| PreviewError::new("config", "json"))?;
    if text_of(map, "protocol")? != "oo.builder.role-visibility/v1" {
        return Err(PreviewError::new("config", "version"));
    }
    let role = text_of(map, "role")?;
    if !role_ok(role) {
        return Err(PreviewError::new("config", "role"));
    }
    let project = text_of(map, "project")?;
    if !mysql_ident(project) {
        return Err(PreviewError::new("ident", "ident"));
    }
    let rows = map
        .get("datasets")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| PreviewError::new("config", "datasets"))?;
    if rows.is_empty() {
        return Err(PreviewError::new("config", "datasets"));
    }
    let mut datasets = Vec::new();
    for row in rows {
        let item = row
            .as_object()
            .ok_or_else(|| PreviewError::new("config", "datasets"))?;
        let dataset = text_of(item, "dataset")?;
        let Some((schema, table)) = dataset.split_once('.') else {
            return Err(PreviewError::new("config", "datasets"));
        };
        if !mysql_ident(schema) || !mysql_ident(table) || dataset.matches('.').count() != 1 {
            return Err(PreviewError::new("config", "datasets"));
        }
        let columns_value = item
            .get("columns")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| PreviewError::new("config", "columns"))?;
        let mut columns = Vec::new();
        for column in columns_value {
            let name = column
                .as_str()
                .ok_or_else(|| PreviewError::new("config", "columns"))?;
            if !mysql_ident(name) {
                return Err(PreviewError::new("ident", "ident"));
            }
            columns.push(name.to_string());
        }
        datasets.push(Dataset {
            schema: schema.to_string(),
            table: table.to_string(),
            columns,
        });
    }
    Ok(Visibility {
        role: role.to_string(),
        project: project.to_string(),
        datasets,
    })
}

async fn dial(
    request: &AclRequest,
    password: &str,
    ca: &[u8],
) -> Result<sqlx::PgConnection, PreviewError> {
    let options = PgConnectOptions::from_explicit()
        .host(&request.host)
        .port(request.port)
        .username(&request.admin_user)
        .password(password)
        .database(&request.database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec())
        .disable_statement_logging();
    match timeout(
        Duration::from_millis(request.connect_timeout_ms),
        options.connect(),
    )
    .await
    {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(_)) => Err(PreviewError::new("connect", "target")),
        Err(_) => Err(PreviewError::new("connect", "connect_timeout")),
    }
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
    fn hidden_comment_is_revoked_and_not_granted() {
        let mapped = vec![
            "shipment_id".to_string(),
            "cost_amount".to_string(),
            "comment".to_string(),
            "seal".to_string(),
        ];
        let visible = vec!["shipment_id".to_string(), "cost_amount".to_string()];
        let plan = plan_columns(&mapped, &visible).expect("plan");
        assert_eq!(plan.grant, visible);
        assert_eq!(plan.hidden, vec!["comment".to_string(), "seal".to_string()]);
    }

    #[test]
    fn a_visible_column_missing_from_the_mapping_is_not_granted() {
        let err = plan_columns(
            &["shipment_id".to_string()],
            &["shipment_id".to_string(), "comment".to_string()],
        )
        .expect_err("absent");
        assert_eq!(err.fact, "mapping_absent");
    }
}
