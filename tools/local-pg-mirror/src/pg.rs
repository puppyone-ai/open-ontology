use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Row};

use crate::error::{parse_sqlstate, MirrorError};
use crate::plan::{simple_ident, Plan};

const DENIED: &[&str] = &[
    "postgres",
    "oo_fixture_admin",
    "oo_pipeline_runtime",
    "oo_target_owner",
];

pub enum SessionKind {
    Admin,
    Runtime,
}

#[allow(clippy::too_many_arguments)]
pub async fn dial(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    database: &str,
    ca_pem: &[u8],
    connect_timeout: Duration,
    session: SessionKind,
) -> Result<PgConnection, MirrorError> {
    let options = PgConnectOptions::from_explicit()
        .host(host)
        .port(port)
        .username(user)
        .password(password)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca_pem.to_vec());
    if options.get_options().is_some() {
        return Err(MirrorError::new("config", "connect_options"));
    }
    let options = options.disable_statement_logging();
    let connect = tokio::time::timeout(connect_timeout, options.connect()).await;
    let mut conn = match connect {
        Ok(Ok(conn)) => conn,
        Ok(Err(err)) => return Err(map_sqlx(err)),
        Err(_) => return Err(MirrorError::new("connect", "connect_timeout")),
    };
    match session {
        SessionKind::Admin => {
            sqlx::query("SET statement_timeout = '120s'")
                .execute(&mut conn)
                .await
                .map_err(map_sqlx)?;
        }
        SessionKind::Runtime => {
            sqlx::query("SET statement_timeout = '30s'")
                .execute(&mut conn)
                .await
                .map_err(map_sqlx)?;
        }
    }
    sqlx::query("SET lock_timeout = '1s'")
        .execute(&mut conn)
        .await
        .map_err(map_sqlx)?;
    sqlx::query("SET search_path = pg_catalog")
        .execute(&mut conn)
        .await
        .map_err(map_sqlx)?;
    Ok(conn)
}

/// Source login. `application_name` is a startup field, not an options string.
/// The snapshot transaction is opened by the caller.
pub async fn dial_source(
    plan: &Plan,
    password: &str,
    ca_pem: &[u8],
    connect_timeout: Duration,
) -> Result<PgConnection, MirrorError> {
    let options = PgConnectOptions::from_explicit()
        .host(&plan.config.host)
        .port(plan.config.port)
        .username(&plan.config.user)
        .password(password)
        .database(&plan.config.database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca_pem.to_vec())
        .application_name("oo-pg-source");
    if options.get_options().is_some() {
        return Err(MirrorError::new("config", "connect_options"));
    }
    let options = options.disable_statement_logging();
    let connect = tokio::time::timeout(connect_timeout, options.connect()).await;
    let mut conn = match connect {
        Ok(Ok(conn)) => conn,
        Ok(Err(err)) => return Err(map_sqlx(err)),
        Err(_) => return Err(MirrorError::new("connect", "connect_timeout")),
    };
    set_text(&mut conn, "statement_timeout", &format!("{}ms", plan.config.statement_timeout_ms)).await?;
    set_text(&mut conn, "lock_timeout", &format!("{}ms", plan.config.lock_timeout_ms)).await?;
    set_text(
        &mut conn,
        "idle_in_transaction_session_timeout",
        &format!("{}ms", plan.config.snapshot_deadline_ms),
    )
    .await?;
    set_text(&mut conn, "default_transaction_read_only", "on").await?;
    set_text(&mut conn, "search_path", "pg_catalog").await?;
    Ok(conn)
}

async fn set_text(conn: &mut PgConnection, name: &str, value: &str) -> Result<(), MirrorError> {
    sqlx::query("SELECT pg_catalog.set_config($1, $2, false)")
        .bind(name)
        .bind(value)
        .execute(&mut *conn)
        .await
        .map_err(map_sqlx)?;
    Ok(())
}

pub async fn prepare_target(conn: &mut PgConnection, plan: &Plan) -> Result<(), MirrorError> {
    let admin = plan.config.target_admin_user.as_str();
    let runtime = plan.config.target_runtime_user.as_str();
    let reader = plan.config.target_reader_user.as_str();
    let database = plan.config.target_database.as_str();
    require_login(conn, runtime, admin).await?;
    require_login(conn, reader, admin).await?;
    oo_pipeline::target_installer::install(&mut *conn)
        .await
        .map_err(map_install)?;
    oo_pipeline::target_installer::install_full_import(&mut *conn)
        .await
        .map_err(map_install)?;
    oo_pipeline::target_installer::verify_full_import_readiness(&mut *conn)
        .await
        .map_err(|_| MirrorError::new("refused", "not_ready"))?;
    grant_member(conn, runtime).await?;
    grant_connect(conn, database, runtime).await?;
    grant_connect(conn, database, reader).await?;
    Ok(())
}

pub async fn grant_reader(conn: &mut PgConnection, reader: &str, tables: &[String]) -> Result<(), MirrorError> {
    if !simple_ident(reader) {
        return Err(MirrorError::new("config", "user"));
    }
    let reader_sql = quote_simple(reader)?;
    let usage = format!("GRANT USAGE ON SCHEMA \"_oo\" TO {reader_sql}");
    sqlx::raw_sql(sqlx::AssertSqlSafe(usage))
        .execute(&mut *conn)
        .await
        .map_err(map_sqlx)?;
    for table in tables {
        let quoted = oo_pipeline::physical_mapping::quote_physical_name(table)
            .map_err(|_| MirrorError::new("config", "physical_name"))?;
        let grant = format!("GRANT SELECT ON TABLE \"_oo\".{quoted} TO {reader_sql}");
        sqlx::raw_sql(sqlx::AssertSqlSafe(grant))
            .execute(&mut *conn)
            .await
            .map_err(map_sqlx)?;
    }
    Ok(())
}

async fn require_login(conn: &mut PgConnection, name: &str, admin: &str) -> Result<(), MirrorError> {
    if !simple_ident(name) || DENIED.contains(&name) || name == admin {
        return Err(MirrorError::new("config", "user"));
    }
    let row = sqlx::query("SELECT rolsuper, rolcanlogin FROM pg_catalog.pg_roles WHERE rolname = $1")
        .bind(name)
        .fetch_optional(&mut *conn)
        .await
        .map_err(map_sqlx)?;
    let Some(row) = row else {
        return Err(MirrorError::new("refused", "role_missing"));
    };
    let superuser: bool = row.try_get(0).map_err(map_sqlx)?;
    let login: bool = row.try_get(1).map_err(map_sqlx)?;
    if superuser || !login {
        return Err(MirrorError::new("refused", "role"));
    }
    Ok(())
}

async fn grant_member(conn: &mut PgConnection, runtime: &str) -> Result<(), MirrorError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members m \
         JOIN pg_catalog.pg_roles granted ON granted.oid = m.roleid \
         JOIN pg_catalog.pg_roles member ON member.oid = m.member \
         WHERE granted.rolname = 'oo_pipeline_runtime' AND member.rolname = $1)",
    )
    .bind(runtime)
    .fetch_one(&mut *conn)
    .await
    .map_err(map_sqlx)?;
    if exists {
        return Ok(());
    }
    let quoted = quote_simple(runtime)?;
    let sql = format!("GRANT oo_pipeline_runtime TO {quoted}");
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *conn)
        .await
        .map_err(map_sqlx)?;
    Ok(())
}

async fn grant_connect(conn: &mut PgConnection, database: &str, user: &str) -> Result<(), MirrorError> {
    if !simple_ident(database) || !simple_ident(user) {
        return Err(MirrorError::new("config", "user"));
    }
    let allowed: bool = sqlx::query_scalar(
        "SELECT pg_catalog.has_database_privilege($1, $2, 'CONNECT')",
    )
    .bind(user)
    .bind(database)
    .fetch_one(&mut *conn)
    .await
    .map_err(map_sqlx)?;
    if allowed {
        return Ok(());
    }
    let sql = format!(
        "GRANT CONNECT ON DATABASE {} TO {}",
        quote_simple(database)?,
        quote_simple(user)?
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *conn)
        .await
        .map_err(map_sqlx)?;
    Ok(())
}

fn quote_simple(name: &str) -> Result<String, MirrorError> {
    if !simple_ident(name) {
        return Err(MirrorError::new("config", "user"));
    }
    Ok(format!("\"{name}\""))
}

pub fn map_source(err: &oo_connectors::SourceError) -> MirrorError {
    let code = match err.kind() {
        oo_connectors::SourceErrorKind::Config => "config",
        oo_connectors::SourceErrorKind::Connect => "connect",
        oo_connectors::SourceErrorKind::Auth => "auth",
        oo_connectors::SourceErrorKind::Tls => "tls",
        oo_connectors::SourceErrorKind::Privilege => "privilege",
        oo_connectors::SourceErrorKind::Version => "version",
        oo_connectors::SourceErrorKind::Budget => "budget",
        oo_connectors::SourceErrorKind::Catalog => "catalog",
        oo_connectors::SourceErrorKind::Identifier => "identifier",
        oo_connectors::SourceErrorKind::Protocol => "protocol",
    };
    let fact = known_fact(err.fact());
    match err.sqlstate().and_then(parse_sqlstate) {
        Some(state) => MirrorError::with_sqlstate(code, fact, state),
        None => MirrorError::new(code, fact),
    }
}

fn known_fact(fact: Option<&str>) -> &'static str {
    match fact {
        Some("generation_reused") => "generation_reused",
        Some("deadline") => "deadline",
        Some("statement_timeout") => "statement_timeout",
        Some("lock_timeout") => "lock_timeout",
        Some("schema_drift") => "schema_drift",
        Some("catalog_identity") => "catalog_identity",
        Some("registration_refused") => "registration_refused",
        Some("write_capable") => "write_capable",
        Some("authentication_failed") => "authentication_failed",
        Some("insufficient_privilege") => "insufficient_privilege",
        Some("tls_handshake") => "tls_handshake",
        Some("io") => "io",
        Some("connection") => "connection",
        Some("database") => "database",
        Some("max_connections") => "max_connections",
        Some("selection") => "selection",
        Some("connect_timeout") => "connect_timeout",
        Some("generation_invalid") => "generation_invalid",
        Some("config_identity") => "config_identity",
        Some("type_policy_identity") => "type_policy_identity",
        Some("principal") => "principal",
        Some("postmaster_start") => "postmaster_start",
        Some("max_bytes") => "max_bytes",
        Some("max_rows") => "max_rows",
        Some("endpoint") => "endpoint",
        Some("credentials") => "credentials",
        Some("ca_pem") => "ca_pem",
        Some("secret_ref") => "secret_ref",
        Some("source_id") => "source_id",
        Some("budget") => "budget",
        Some("driver") => "driver",
        Some("protocol") => "protocol",
        Some("read_node") => "read_node",
        _ => "source",
    }
}

pub fn map_sqlx(err: sqlx::Error) -> MirrorError {
    if let Some(db) = err.as_database_error() {
        let state = db.code().map(|code| code.into_owned()).unwrap_or_default();
        if let Some(sqlstate) = parse_sqlstate(&state) {
            let fact = match state.as_str() {
                "28P01" | "28000" => "authentication_failed",
                "42501" => "insufficient_privilege",
                "3D000" => "database",
                "57014" => "statement_timeout",
                "55P03" => "lock_timeout",
                _ if state.starts_with("08") => "connection",
                _ => "sql",
            };
            return MirrorError::with_sqlstate("database", fact, sqlstate);
        }
        return MirrorError::new("database", "sql");
    }
    match err {
        sqlx::Error::Tls(_) => MirrorError::new("tls", "tls_handshake"),
        sqlx::Error::Io(_) => MirrorError::new("connect", "io"),
        sqlx::Error::PoolTimedOut => MirrorError::new("connect", "connect_timeout"),
        _ => MirrorError::new("connect", "driver"),
    }
}

fn map_install(err: oo_pipeline::target_installer::InstallError) -> MirrorError {
    match err.sqlstate().and_then(parse_sqlstate) {
        Some(state) => MirrorError::with_sqlstate("refused", "install", state),
        None => MirrorError::new("refused", "install"),
    }
}
