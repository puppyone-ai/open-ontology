//! sqlx helpers. Statements are fixed text or names that already matched `pt_`/`pc_`.

use std::fs;

use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};

use crate::fixture::{self, CUSTOMERS, ORDERS};
use crate::live::{Live, LOGIN, READER, RUNTIME, SOURCE_DB, SOURCE_ID, WRITER};
use crate::stop::{disk, Stop};

pub async fn dial(live: &Live, user: &str, password: &str, database: &str) -> Result<PgConnection, Stop> {
    let options = PgConnectOptions::new()
        .host(&live.host)
        .port(live.port)
        .username(user)
        .password(password)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(live.ca.clone());
    PgConnection::connect_with(&options.disable_statement_logging())
        .await
        .map_err(|_| Stop::new("dial", format!("user={user} db={database}")))
}

pub async fn exec_sql(conn: &mut PgConnection, sql: &str) -> Result<(), Stop> {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
        .map_err(|err| Stop::new("sql", sqlstate(&err)))?;
    Ok(())
}

pub async fn scalar_text(conn: &mut PgConnection, sql: &str) -> Result<String, Stop> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| Stop::new("sql", sqlstate(&err)))
}

pub async fn reset_cluster(admin: &mut PgConnection) -> Result<(), Stop> {
    for database in [SOURCE_DB, crate::live::TARGET_DB] {
        exec_sql(admin, &format!("DROP DATABASE IF EXISTS {database} WITH (FORCE)")).await?;
    }
    for role in [
        READER,
        WRITER,
        RUNTIME,
        LOGIN,
        "oo_r_clerk",
        "oo_r_peek",
        "oo_pipeline_runtime",
        "oo_target_owner",
    ] {
        let _ = exec_sql(admin, &format!("DROP ROLE IF EXISTS {role}")).await;
    }
    Ok(())
}

pub async fn create_roles(live: &Live, admin: &mut PgConnection) -> Result<(), Stop> {
    let roles = [
        (READER, live.reader_password.as_str(), "INHERIT"),
        (WRITER, live.writer_password.as_str(), "INHERIT"),
        (RUNTIME, live.runtime_password.as_str(), "INHERIT"),
        (LOGIN, live.login_password.as_str(), "NOINHERIT"),
    ];
    for (name, password, inherit) in roles {
        exec_sql(
            admin,
            &format!(
                "CREATE ROLE {name} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION {inherit}"
            ),
        )
        .await?;
    }
    Ok(())
}

pub async fn create_database(admin: &mut PgConnection, name: &str) -> Result<(), Stop> {
    exec_sql(
        admin,
        &format!("CREATE DATABASE {name} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'"),
    )
    .await
}

pub async fn record_server(live: &Live, conn: &mut PgConnection) -> Result<(), Stop> {
    let ssl = scalar_text(conn, "SELECT ssl::text FROM pg_stat_ssl WHERE pid = pg_backend_pid()").await?;
    let version = scalar_text(conn, "SHOW server_version").await?;
    if ssl != "true" && ssl != "t" {
        return Err(Stop::new("tls", "ssl is not true"));
    }
    fs::write(live.evidence.join("server.txt"), format!("ssl={ssl}\nserver_version={version}\n"))
        .map_err(|err| disk("server", &err))
}

pub async fn load_fixture(live: &Live) -> Result<(), Stop> {
    let sql = fs::read_to_string(&live.fixture).map_err(|err| disk("fixture", &err))?;
    for needle in fixture::SQL_NEEDLES {
        if !sql.contains(needle) {
            return Err(Stop::new("fixture", "typed gold needle is absent from the SQL file"));
        }
    }
    if !sql.contains(fixture::MARKER) {
        return Err(Stop::new("fixture", "comment marker is absent"));
    }
    let mut source = dial(live, &live.admin_user, &live.admin_password, SOURCE_DB).await?;
    exec_sql(&mut source, &sql).await?;
    Ok(())
}

pub async fn reader_denied(live: &Live) -> Result<(), Stop> {
    let mut reader = dial(live, READER, &live.reader_password, SOURCE_DB).await?;
    let denied = expect_state(
        &mut reader,
        "INSERT INTO shop.customer (customer_id, display_name, active) VALUES (9, 'no', true)",
        "42501",
    )
    .await?;
    if denied {
        Ok(())
    } else {
        Err(Stop::new("source-write", "reader insert was not 42501"))
    }
}

pub async fn compare_source_gold(conn: &mut PgConnection) -> Result<(), Stop> {
    let customers = text_matrix(
        conn,
        "SELECT customer_id::text, display_name, active::text, private_note FROM shop.customer ORDER BY customer_id",
        4,
    )
    .await?;
    let orders = text_matrix(
        conn,
        "SELECT order_id::text, customer_id::text, memo, posted::text, amount::text, clerk_note FROM shop.order_doc ORDER BY order_id",
        6,
    )
    .await?;
    if customers != customer_matrix() || orders != order_matrix() {
        return Err(Stop::new("fixture", "source rows differ from typed gold"));
    }
    Ok(())
}

fn customer_matrix() -> Vec<Vec<Option<String>>> {
    CUSTOMERS
        .iter()
        .map(|row| {
            vec![
                Some(row.id.to_string()),
                row.display.map(str::to_string),
                Some(row.active.to_string()),
                row.private_note.map(str::to_string),
            ]
        })
        .collect()
}

fn order_matrix() -> Vec<Vec<Option<String>>> {
    ORDERS
        .iter()
        .map(|row| {
            vec![
                Some(row.id.to_string()),
                Some(row.customer_id.to_string()),
                row.memo.map(str::to_string),
                Some(row.posted.to_string()),
                Some(row.amount.to_string()),
                row.clerk_note.map(str::to_string),
            ]
        })
        .collect()
}

pub async fn expect_receipt(conn: &mut PgConnection, run_id: &str, physical: &str, digest: &str) -> Result<(), Stop> {
    let stored: (String, i64) = sqlx::query_as(sqlx::AssertSqlSafe(
        "SELECT canonical_digest, applied_rows FROM _oo.full_import_table WHERE run_id = $1::uuid AND physical_name = $2"
            .to_string(),
    ))
    .bind(run_id)
    .bind(physical)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| Stop::new("receipt", sqlstate(&err)))?;
    if stored.0 == digest && stored.1 == 3 {
        Ok(())
    } else {
        Err(Stop::new("receipt", "digest or rows"))
    }
}

pub async fn expect_mapped_table(conn: &mut PgConnection, name: &str, physical: &str) -> Result<(), Stop> {
    let mapped: String = sqlx::query_scalar(sqlx::AssertSqlSafe(
        "SELECT physical_name FROM _oo.physical_mappings WHERE source = 'a226src' AND level = 't' AND logical_namespace = 'shop' AND logical_table = $1 AND logical_column = ''"
            .to_string(),
    ))
    .bind(name)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| Stop::new("mapping", sqlstate(&err)))?;
    if mapped == physical {
        Ok(())
    } else {
        Err(Stop::new("mapping", "table"))
    }
}

pub async fn expect_mapped_column(
    conn: &mut PgConnection,
    table: &str,
    column: &str,
    physical: &str,
) -> Result<(), Stop> {
    let mapped: String = sqlx::query_scalar(sqlx::AssertSqlSafe(
        "SELECT physical_name FROM _oo.physical_mappings WHERE source = 'a226src' AND level = 'c' AND logical_namespace = 'shop' AND logical_table = $1 AND logical_column = $2"
            .to_string(),
    ))
    .bind(table)
    .bind(column)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| Stop::new("mapping", sqlstate(&err)))?;
    if mapped == physical {
        Ok(())
    } else {
        Err(Stop::new("mapping", "column"))
    }
}

pub async fn mapping_identity(conn: &mut PgConnection) -> Result<(String, i64), Stop> {
    let rows: Vec<(String, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(
        format!("SELECT DISTINCT config_digest, run_generation FROM _oo.physical_mappings WHERE source = '{SOURCE_ID}'"),
    ))
    .fetch_all(&mut *conn)
    .await
    .map_err(|err| Stop::new("mapping", sqlstate(&err)))?;
    if rows.len() == 1 && rows[0].0.len() == 64 {
        Ok((rows[0].0.clone(), rows[0].1))
    } else {
        Err(Stop::new("mapping", "config digest was not one value"))
    }
}

pub async fn shadow_values(conn: &mut PgConnection, table: &str, column: &str) -> Result<Vec<Option<String>>, Stop> {
    if !physical_ok(table, "pt_") || !physical_ok(column, "pc_") {
        return Err(Stop::new("rows", "physical name"));
    }
    let sql = format!("SELECT \"{column}\"::text FROM _oo.\"{table}\" ORDER BY \"_oo_pos\"");
    let mut values = text_column(conn, &sql).await?;
    for value in &mut values {
        *value = value.take().map(|text| norm_bool(&text));
    }
    Ok(values)
}

pub async fn privilege_text(conn: &mut PgConnection) -> Result<String, Stop> {
    let row = sqlx::query(sqlx::AssertSqlSafe(
        "SELECT has_schema_privilege('oo_a226_login', '_oo', 'USAGE') AS usage, has_table_privilege('oo_a226_login', '_oo.physical_mappings', 'SELECT') AS mapping_select, has_function_privilege('oo_a226_login', '_oo.lookup_physical_mapping(text, text, bigint, text, text, text, text)', 'EXECUTE') AS mapping_execute".to_string(),
    ))
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| Stop::new("mapping-grant", sqlstate(&err)))?;
    let usage: bool = row.try_get("usage").map_err(|_| Stop::new("mapping-grant", "usage"))?;
    let select: bool = row
        .try_get("mapping_select")
        .map_err(|_| Stop::new("mapping-grant", "select"))?;
    let execute: bool = row
        .try_get("mapping_execute")
        .map_err(|_| Stop::new("mapping-grant", "execute"))?;
    Ok(format!("{usage},{select},{execute}"))
}

pub async fn grant_mapping(conn: &mut PgConnection) -> Result<(), Stop> {
    exec_sql(
        conn,
        "GRANT USAGE ON SCHEMA _oo TO oo_a226_login; GRANT SELECT ON TABLE _oo.physical_mappings TO oo_a226_login; GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO oo_a226_login",
    )
    .await
}

pub async fn reltuples(conn: &mut PgConnection, physical: &str) -> Result<String, Stop> {
    if !physical_ok(physical, "pt_") {
        return Err(Stop::new("reltuples", "physical name"));
    }
    let sql = format!(
        "SELECT c.reltuples::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname = '{physical}'"
    );
    scalar_text(conn, &sql).await
}

pub fn physical_ok(name: &str, prefix: &str) -> bool {
    let Some(rest) = name.strip_prefix(prefix) else {
        return false;
    };
    rest.len() == 60 && rest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn text_of(value: &Value, key: &str) -> Result<String, Stop> {
    value[key]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Stop::new("json", key))
}

async fn text_column(conn: &mut PgConnection, sql: &str) -> Result<Vec<Option<String>>, Stop> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&mut *conn)
        .await
        .map_err(|err| Stop::new("sql", sqlstate(&err)))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(row.try_get(0).map_err(|_| Stop::new("sql", "text column"))?);
    }
    Ok(out)
}

async fn text_matrix(conn: &mut PgConnection, sql: &str, width: usize) -> Result<Vec<Vec<Option<String>>>, Stop> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&mut *conn)
        .await
        .map_err(|err| Stop::new("sql", sqlstate(&err)))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut cells = Vec::with_capacity(width);
        for index in 0..width {
            let value: Option<String> = row.try_get(index).map_err(|_| Stop::new("sql", "matrix"))?;
            cells.push(value.map(|text| norm_bool(&text)));
        }
        out.push(cells);
    }
    Ok(out)
}

async fn expect_state(conn: &mut PgConnection, sql: &str, state: &str) -> Result<bool, Stop> {
    match sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string())).execute(&mut *conn).await {
        Ok(_) => Ok(false),
        Err(err) => Ok(sqlstate(&err) == state),
    }
}

fn norm_bool(text: &str) -> String {
    match text {
        "t" | "true" => "true".to_string(),
        "f" | "false" => "false".to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn sqlstate(err: &sqlx::Error) -> String {
    match err {
        sqlx::Error::Database(db) => db.code().map(|code| code.to_string()).unwrap_or_else(|| "db".to_string()),
        _ => "transport".to_string(),
    }
}
