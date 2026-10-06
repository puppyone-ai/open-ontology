//! Explicit operator maintenance for mapped shadow statistics.
//!
//! The caller passes one target connection, the mapping identity, and a
//! bounded list of logical tables. Physical names come from
//! [`crate::physical_mapping::plan_physical_mapping`] and
//! `_oo.lookup_physical_mapping`. This module quotes those stored names and
//! runs `ANALYZE` on the `_oo` shadow. It does not import, does not touch a
//! source database, and does not publish.

use std::collections::BTreeSet;
use std::fmt;

use sqlx::postgres::PgConnection;
use sqlx::{Connection, Row};

use crate::physical_mapping::{
    plan_physical_mapping, quote_physical_name, Level, MappingRequest, PlanError,
};

/// Protocol name stored on a receipt.
pub const PROTOCOL: &str = "target-statistics/v1";

/// Largest logical-table list one call accepts.
pub const MAX_TABLES: usize = 8;

/// Smallest statement or lock timeout, in milliseconds.
pub const MIN_TIMEOUT_MS: u32 = 1;

/// Largest statement or lock timeout, in milliseconds.
pub const MAX_TIMEOUT_MS: u32 = 30_000;

const SHADOW_SCHEMA: &str = "_oo";
const RUNTIME_ROLE: &str = "oo_pipeline_runtime";
const SHADOW_OWNER: &str = "oo_target_owner";

/// One logical table. The caller does not pass a physical name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicalTableRef {
    /// Mapping namespace.
    pub namespace: String,
    /// Mapping table name.
    pub name: String,
}

/// What the trusted operator asks to analyze.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatisticsRequest {
    /// Policy project. Must equal [`Self::source`].
    pub project: String,
    /// `physical_mappings.source`.
    pub source: String,
    /// Mapping config digest, 64 lowercase hex characters.
    pub config_digest: String,
    /// Mapping `run_generation`. Must be the stored maximum for this source
    /// and digest.
    pub source_generation: i64,
    /// Logical tables, in analyze order. One through [`MAX_TABLES`].
    pub tables: Vec<LogicalTableRef>,
    /// Local `statement_timeout` for this maintenance transaction.
    pub statement_timeout_ms: u32,
    /// Local `lock_timeout` for this maintenance transaction.
    pub lock_timeout_ms: u32,
}

/// Measured `pg_class.reltuples` for one mapped shadow.
///
/// `estimated_rows` is the half-away-from-zero rounding of that float.
/// It is not a claim that the table contains that many rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableEstimate {
    /// Logical namespace from the request.
    pub logical_namespace: String,
    /// Logical table from the request.
    pub logical_table: String,
    /// Stored physical name that was analyzed.
    pub physical_name: String,
    /// `f32::to_bits` of `reltuples` after `ANALYZE`.
    pub reltuples_bits: u32,
    /// Rounded estimate. Not an exact count.
    pub estimated_rows: u64,
}

/// Receipt of one committed maintenance transaction.
///
/// `published` and `estimate_is_exact` are always false.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatisticsReceipt {
    /// [`PROTOCOL`].
    pub protocol: &'static str,
    /// Always false. This call does not publish.
    pub published: bool,
    /// Always false. `ANALYZE` does not prove an exact row count.
    pub estimate_is_exact: bool,
    /// Project from the request.
    pub project: String,
    /// Source from the request.
    pub source: String,
    /// Config digest from the request.
    pub config_digest: String,
    /// Source generation that was current.
    pub source_generation: i64,
    /// Timeout that was set locally.
    pub statement_timeout_ms: u32,
    /// Timeout that was set locally.
    pub lock_timeout_ms: u32,
    /// One entry per requested table, in request order.
    pub tables: Vec<TableEstimate>,
    /// Whether the caller can send another statement on this connection.
    pub disposition: ConnectionDisposition,
}

/// What the caller should do with the connection after the call returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionDisposition {
    /// No transaction is open. Another statement can use this connection.
    Reusable,
    /// The session is not idle. Open a new connection.
    FreshConnection,
}

/// Why maintenance did not return a receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatisticsError {
    /// Project, source, digest, or generation is not a closed identity.
    Identity,
    /// The table list or a logical name is outside the bound.
    Bound,
    /// A timeout is outside [`MIN_TIMEOUT_MS`]..=[`MAX_TIMEOUT_MS`].
    TimeoutBound,
    /// The table list is empty.
    EmptyTables,
    /// The same namespace and table appears twice.
    DuplicateTable,
    /// The connection already has a transaction open.
    OpenTransaction,
    /// `current_user` is `oo_pipeline_runtime` or a non-superuser member.
    RuntimeCaller,
    /// The session or the role default is read only, or the server is in recovery.
    ReadOnlyCaller,
    /// The caller cannot maintain the mapped shadow.
    RoleDenied,
    /// `lock_timeout` cancelled the statement. SQLSTATE `55P03`.
    LockTimeout,
    /// `statement_timeout` cancelled the statement. SQLSTATE `57014`.
    StatementTimeout,
    /// `source_generation` is not `max(run_generation)` for this source and digest.
    StaleGeneration,
    /// Lookup raised the absent-mapping condition.
    MissingTable,
    /// The mapped `_oo` relation is not there.
    MissingRelation,
    /// The stored name or the relation is not the mapped ordinary shadow.
    ForgedRelation,
    /// `reltuples` is still the unknown sentinel after `ANALYZE`.
    EstimateUnknown,
    /// `reltuples` is not a finite value of at least zero.
    EstimateUnusable,
    /// Commit or rollback did not leave a known session state.
    TransactionIncomplete,
    /// PostgreSQL rejected a statement. The server text is discarded.
    Database {
        /// SQLSTATE, or empty when the driver has none.
        sqlstate: String,
    },
}

/// Failure plus the connection disposition after rollback or the probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaintenanceError {
    /// Why the call failed.
    pub kind: StatisticsError,
    /// Connection state after this call tried to finish its own transaction.
    pub disposition: ConnectionDisposition,
}

impl fmt::Display for StatisticsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity => formatter.write_str("target statistics identity is closed"),
            Self::Bound => formatter.write_str("target statistics bound is closed"),
            Self::TimeoutBound => formatter.write_str("target statistics timeout is closed"),
            Self::EmptyTables => formatter.write_str("target statistics table list is empty"),
            Self::DuplicateTable => formatter.write_str("target statistics table is duplicated"),
            Self::OpenTransaction => {
                formatter.write_str("target statistics connection already has a transaction")
            }
            Self::RuntimeCaller => formatter.write_str("target statistics runtime caller is refused"),
            Self::ReadOnlyCaller => formatter.write_str("target statistics read-only caller is refused"),
            Self::RoleDenied => formatter.write_str("target statistics caller cannot maintain"),
            Self::LockTimeout => formatter.write_str("target statistics lock timed out"),
            Self::StatementTimeout => formatter.write_str("target statistics statement timed out"),
            Self::StaleGeneration => formatter.write_str("target statistics generation is not current"),
            Self::MissingTable => formatter.write_str("target statistics mapping is absent"),
            Self::MissingRelation => formatter.write_str("target statistics relation is absent"),
            Self::ForgedRelation => formatter.write_str("target statistics relation is not the mapped shadow"),
            Self::EstimateUnknown => formatter.write_str("target statistics estimate is unknown"),
            Self::EstimateUnusable => formatter.write_str("target statistics estimate is not usable"),
            Self::TransactionIncomplete => {
                formatter.write_str("target statistics transaction did not complete")
            }
            Self::Database { sqlstate } => {
                if sqlstate_ok(sqlstate) {
                    write!(formatter, "postgres sqlstate={sqlstate}")
                } else {
                    formatter.write_str("postgres error")
                }
            }
        }
    }
}

impl fmt::Display for MaintenanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = &self.kind;
        let disposition = disposition_text(self.disposition);
        write!(formatter, "{kind} ({disposition})")
    }
}

impl std::error::Error for StatisticsError {}
impl std::error::Error for MaintenanceError {}

struct Planned {
    namespace: String,
    table: String,
    physical: String,
    quoted: String,
    oid: i64,
}

macro_rules! abort_with {
    ($conn:expr, $tx:expr, $kind:expr) => {{
        let rolled_back = $tx.rollback().await.is_ok();
        let disposition = if rolled_back {
            probe($conn).await
        } else {
            ConnectionDisposition::FreshConnection
        };
        MaintenanceError {
            kind: $kind,
            disposition,
        }
    }};
}

/// Analyze the mapped shadows and return the measured estimates.
///
/// The connection must be idle. The function opens one transaction, sets the
/// local timeouts, checks the identity, and analyzes only the quoted physical
/// names from the mapping. On failure it rolls that transaction back when it
/// still owns it.
pub async fn analyze_mapped_shadows(
    conn: &mut PgConnection,
    request: &StatisticsRequest,
) -> Result<StatisticsReceipt, MaintenanceError> {
    if let Err(kind) = validate(request) {
        return Err(closed(kind));
    }
    ensure_idle(conn).await?;
    ensure_operator(conn).await?;
    // `begin`'s `Result` keeps the connection borrow until that value is
    // dropped, so a failed begin does not probe this connection.
    let mut tx = conn.begin().await.map_err(|err| MaintenanceError {
        kind: driver_kind(&err),
        disposition: ConnectionDisposition::FreshConnection,
    })?;
    if let Err(kind) = apply_local_limits(&mut tx, request).await {
        return Err(abort_with!(conn, tx, kind));
    }
    if let Err(kind) = ensure_operator_tx(&mut tx).await {
        return Err(abort_with!(conn, tx, kind));
    }
    let planned = match resolve_all(&mut tx, request).await {
        Ok(planned) => planned,
        Err(kind) => return Err(abort_with!(conn, tx, kind)),
    };
    if let Err(kind) = analyze_all(&mut tx, &planned).await {
        return Err(abort_with!(conn, tx, kind));
    }
    let tables = match read_estimates(&mut tx, &planned).await {
        Ok(tables) => tables,
        Err(kind) => return Err(abort_with!(conn, tx, kind)),
    };
    if let Err(err) = tx.commit().await {
        return Err(from_driver(conn, err).await);
    }
    let disposition = probe(conn).await;
    Ok(StatisticsReceipt {
        protocol: PROTOCOL,
        published: false,
        estimate_is_exact: false,
        project: request.project.clone(),
        source: request.source.clone(),
        config_digest: request.config_digest.clone(),
        source_generation: request.source_generation,
        statement_timeout_ms: request.statement_timeout_ms,
        lock_timeout_ms: request.lock_timeout_ms,
        tables,
        disposition,
    })
}

fn validate(request: &StatisticsRequest) -> Result<(), StatisticsError> {
    if request.project != request.source {
        return Err(StatisticsError::Identity);
    }
    if !timeout_ok(request.statement_timeout_ms) || !timeout_ok(request.lock_timeout_ms) {
        return Err(StatisticsError::TimeoutBound);
    }
    if request.tables.is_empty() {
        return Err(StatisticsError::EmptyTables);
    }
    if request.tables.len() > MAX_TABLES {
        return Err(StatisticsError::Bound);
    }
    let mut seen = BTreeSet::new();
    for table in &request.tables {
        if !seen.insert((table.namespace.clone(), table.name.clone())) {
            return Err(StatisticsError::DuplicateTable);
        }
        plan_table(request, table).map_err(plan_error)?;
    }
    Ok(())
}

fn plan_table(request: &StatisticsRequest, table: &LogicalTableRef) -> Result<(), PlanError> {
    plan_physical_mapping(&MappingRequest {
        source: request.source.clone(),
        config_digest: request.config_digest.clone(),
        run_generation: request.source_generation,
        level: Level::Table,
        logical_namespace: table.namespace.clone(),
        logical_table: table.name.clone(),
        logical_column: String::new(),
    })
    .map(|_| ())
}

fn plan_error(err: PlanError) -> StatisticsError {
    match err {
        PlanError::ClosedLogicalName => StatisticsError::Bound,
        PlanError::NotPhysicalName => StatisticsError::ForgedRelation,
        PlanError::ClosedSource | PlanError::ClosedConfig | PlanError::ClosedGeneration => {
            StatisticsError::Identity
        }
    }
}

fn timeout_ok(value: u32) -> bool {
    (MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&value)
}

async fn ensure_idle(conn: &mut PgConnection) -> Result<(), MaintenanceError> {
    match transaction_open(conn).await {
        Ok(false) => Ok(()),
        Ok(true) => Err(closed(StatisticsError::OpenTransaction)),
        Err(err) => Err(from_driver(conn, err).await),
    }
}

async fn transaction_open(conn: &mut PgConnection) -> Result<bool, sqlx::Error> {
    // Simple query. The extended protocol stamps the transaction at Bind and
    // the statement at Execute, so this comparison is true on an idle
    // connection. A simple Query stamps both together unless an earlier
    // command already opened the transaction.
    let row = sqlx::raw_sql(sqlx::AssertSqlSafe(
        "SELECT pg_catalog.statement_timestamp() > pg_catalog.transaction_timestamp()",
    ))
    .fetch_one(&mut *conn)
    .await?;
    row.try_get(0)
}

async fn ensure_operator(conn: &mut PgConnection) -> Result<(), MaintenanceError> {
    match operator_kind(conn).await {
        Ok(None) => Ok(()),
        Ok(Some(kind)) => Err(closed(kind)),
        Err(err) => Err(from_driver(conn, err).await),
    }
}

async fn ensure_operator_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), StatisticsError> {
    match operator_kind(tx).await {
        Ok(None) => Ok(()),
        Ok(Some(kind)) => Err(kind),
        Err(err) => Err(driver_kind(&err)),
    }
}

async fn operator_kind(conn: &mut PgConnection) -> Result<Option<StatisticsError>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT current_setting('transaction_read_only') = 'on', \
                current_setting('default_transaction_read_only') = 'on', \
                pg_catalog.pg_is_in_recovery(), \
                current_user::text = $1 \
                OR ( \
                    NOT role.rolsuper \
                    AND pg_catalog.pg_has_role(current_user, $1, 'member') \
                ) \
         FROM pg_catalog.pg_roles AS role \
         WHERE role.rolname = current_user",
    )
    .bind(RUNTIME_ROLE)
    .fetch_one(&mut *conn)
    .await?;
    let read_only: bool = row.try_get(0).unwrap_or(true);
    let default_read_only: bool = row.try_get(1).unwrap_or(true);
    let recovery: bool = row.try_get(2).unwrap_or(true);
    let runtime: bool = row.try_get(3).unwrap_or(true);
    if runtime {
        return Ok(Some(StatisticsError::RuntimeCaller));
    }
    if read_only || default_read_only || recovery {
        return Ok(Some(StatisticsError::ReadOnlyCaller));
    }
    Ok(None)
}

async fn apply_local_limits(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request: &StatisticsRequest,
) -> Result<(), StatisticsError> {
    set_local(tx, "statement_timeout", &timeout_text(request.statement_timeout_ms)).await?;
    set_local(tx, "lock_timeout", &timeout_text(request.lock_timeout_ms)).await?;
    set_local(tx, "search_path", "pg_catalog").await?;
    Ok(())
}

fn timeout_text(milliseconds: u32) -> String {
    format!("{milliseconds}ms")
}

async fn set_local(conn: &mut PgConnection, name: &str, value: &str) -> Result<(), StatisticsError> {
    sqlx::query("SELECT set_config($1, $2, true)")
        .bind(name)
        .bind(value)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(|err| driver_kind(&err))
}

async fn resolve_all(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request: &StatisticsRequest,
) -> Result<Vec<Planned>, StatisticsError> {
    ensure_current_generation(tx, request).await?;
    let mut planned = Vec::with_capacity(request.tables.len());
    for table in &request.tables {
        planned.push(resolve_one(tx, request, table).await?);
    }
    Ok(planned)
}

async fn ensure_current_generation(
    conn: &mut PgConnection,
    request: &StatisticsRequest,
) -> Result<(), StatisticsError> {
    let current: Option<i64> = sqlx::query_scalar(
        "SELECT max(run_generation) FROM _oo.physical_mappings \
         WHERE source = $1 AND config_digest = $2",
    )
    .bind(&request.source)
    .bind(&request.config_digest)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| driver_kind(&err))?;
    if current == Some(request.source_generation) {
        Ok(())
    } else {
        Err(StatisticsError::StaleGeneration)
    }
}

async fn resolve_one(
    conn: &mut PgConnection,
    request: &StatisticsRequest,
    table: &LogicalTableRef,
) -> Result<Planned, StatisticsError> {
    let plan = plan_physical_mapping(&MappingRequest {
        source: request.source.clone(),
        config_digest: request.config_digest.clone(),
        run_generation: request.source_generation,
        level: Level::Table,
        logical_namespace: table.namespace.clone(),
        logical_table: table.name.clone(),
        logical_column: String::new(),
    })
    .map_err(plan_error)?;
    let stored: String = sqlx::query_scalar(
        "SELECT _oo.lookup_physical_mapping($1, $2, $3, 'table', $4, $5, '')",
    )
    .bind(&request.source)
    .bind(&request.config_digest)
    .bind(request.source_generation)
    .bind(&table.namespace)
    .bind(&table.name)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| driver_kind(&err))?;
    if stored != plan.physical_name {
        return Err(StatisticsError::ForgedRelation);
    }
    let quoted = quote_physical_name(&stored).map_err(plan_error)?;
    let row = sqlx::query(
        "SELECT class.oid::int8, class.relkind::text, pg_catalog.pg_get_userbyid(class.relowner) \
         FROM pg_catalog.pg_class AS class \
         JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace \
         WHERE namespace.nspname = $1 AND class.relname = $2",
    )
    .bind(SHADOW_SCHEMA)
    .bind(&stored)
    .fetch_all(&mut *conn)
    .await
    .map_err(|err| driver_kind(&err))?;
    if row.is_empty() {
        return Err(StatisticsError::MissingRelation);
    }
    if row.len() != 1 {
        return Err(StatisticsError::ForgedRelation);
    }
    let oid: i64 = row[0].try_get(0).map_err(|_| StatisticsError::ForgedRelation)?;
    let relkind: String = row[0].try_get(1).map_err(|_| StatisticsError::ForgedRelation)?;
    let owner: String = row[0].try_get(2).map_err(|_| StatisticsError::ForgedRelation)?;
    if relkind != "r" || owner != SHADOW_OWNER {
        return Err(StatisticsError::ForgedRelation);
    }
    let allowed: bool = sqlx::query_scalar("SELECT pg_catalog.has_table_privilege($1::oid, 'MAINTAIN')")
        .bind(oid)
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| driver_kind(&err))?;
    if !allowed {
        return Err(StatisticsError::RoleDenied);
    }
    Ok(Planned {
        namespace: table.namespace.clone(),
        table: table.name.clone(),
        physical: stored,
        quoted,
        oid,
    })
}

async fn analyze_all(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    planned: &[Planned],
) -> Result<(), StatisticsError> {
    for table in planned {
        let sql = format!("ANALYZE \"_oo\".{}", table.quoted);
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(&mut **tx)
            .await
            .map(|_| ())
            .map_err(|err| driver_kind(&err))?;
    }
    Ok(())
}

async fn read_estimates(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    planned: &[Planned],
) -> Result<Vec<TableEstimate>, StatisticsError> {
    let mut tables = Vec::with_capacity(planned.len());
    for table in planned {
        let value: f32 = sqlx::query_scalar(
            "SELECT class.reltuples \
             FROM pg_catalog.pg_class AS class \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace \
             WHERE namespace.nspname = $1 AND class.relname = $2 AND class.relkind = 'r' \
               AND class.oid = $3::oid",
        )
        .bind(SHADOW_SCHEMA)
        .bind(&table.physical)
        .bind(table.oid)
        .fetch_one(&mut **tx)
        .await
        .map_err(|err| driver_kind(&err))?;
        let estimated_rows = classify_reltuples(value)?;
        tables.push(TableEstimate {
            logical_namespace: table.namespace.clone(),
            logical_table: table.table.clone(),
            physical_name: table.physical.clone(),
            reltuples_bits: value.to_bits(),
            estimated_rows,
        });
    }
    Ok(tables)
}

fn classify_reltuples(value: f32) -> Result<u64, StatisticsError> {
    if !value.is_finite() {
        return Err(StatisticsError::EstimateUnusable);
    }
    if value < 0.0 {
        return if value.to_bits() == (-1.0f32).to_bits() {
            Err(StatisticsError::EstimateUnknown)
        } else {
            Err(StatisticsError::EstimateUnusable)
        };
    }
    let rounded = f64::from(value).round();
    if !(0.0..18_446_744_073_709_551_616.0).contains(&rounded) {
        return Err(StatisticsError::EstimateUnusable);
    }
    Ok(rounded as u64)
}

async fn from_driver(conn: &mut PgConnection, err: sqlx::Error) -> MaintenanceError {
    let kind = driver_kind(&err);
    let disposition = if sqlstate_of(&err) == "25P02" {
        ConnectionDisposition::FreshConnection
    } else {
        probe(conn).await
    };
    MaintenanceError { kind, disposition }
}

fn driver_kind(err: &sqlx::Error) -> StatisticsError {
    let sqlstate = sqlstate_of(err);
    match sqlstate.as_str() {
        "55P03" => StatisticsError::LockTimeout,
        "57014" => StatisticsError::StatementTimeout,
        "OO024" => StatisticsError::MissingTable,
        "42501" => StatisticsError::RoleDenied,
        "25P02" => StatisticsError::TransactionIncomplete,
        _ => StatisticsError::Database { sqlstate },
    }
}

fn sqlstate_of(err: &sqlx::Error) -> String {
    err.as_database_error()
        .and_then(|db| db.code().map(|code| code.into_owned()))
        .unwrap_or_default()
}

async fn probe(conn: &mut PgConnection) -> ConnectionDisposition {
    match transaction_open(conn).await {
        Ok(false) => ConnectionDisposition::Reusable,
        Ok(true) | Err(_) => ConnectionDisposition::FreshConnection,
    }
}

fn closed(kind: StatisticsError) -> MaintenanceError {
    MaintenanceError {
        kind,
        disposition: ConnectionDisposition::Reusable,
    }
}

fn disposition_text(disposition: ConnectionDisposition) -> &'static str {
    match disposition {
        ConnectionDisposition::Reusable => "connection reusable",
        ConnectionDisposition::FreshConnection => "fresh connection required",
    }
}

fn sqlstate_ok(value: &str) -> bool {
    value.len() == 5 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> StatisticsRequest {
        StatisticsRequest {
            project: "a219src".to_string(),
            source: "a219src".to_string(),
            config_digest: "ab".repeat(32),
            source_generation: 11,
            tables: vec![LogicalTableRef {
                namespace: "app".to_string(),
                name: "party".to_string(),
            }],
            statement_timeout_ms: 5_000,
            lock_timeout_ms: 5_000,
        }
    }

    #[test]
    fn closed_inputs_fail_before_a_connection() {
        assert!(validate(&request()).is_ok());
        let mut mismatched = request();
        mismatched.project = "other".to_string();
        assert_eq!(validate(&mismatched), Err(StatisticsError::Identity));
        let mut stale_shape = request();
        stale_shape.source_generation = 0;
        assert_eq!(validate(&stale_shape), Err(StatisticsError::Identity));
        let mut timeout = request();
        timeout.lock_timeout_ms = 0;
        assert_eq!(validate(&timeout), Err(StatisticsError::TimeoutBound));
        timeout.lock_timeout_ms = MAX_TIMEOUT_MS + 1;
        assert_eq!(validate(&timeout), Err(StatisticsError::TimeoutBound));
        let mut empty = request();
        empty.tables.clear();
        assert_eq!(validate(&empty), Err(StatisticsError::EmptyTables));
        let mut duplicate = request();
        duplicate.tables.push(duplicate.tables[0].clone());
        assert_eq!(validate(&duplicate), Err(StatisticsError::DuplicateTable));
        let mut forged = request();
        forged.tables[0].name = "party\"; ANALYZE pg_catalog.pg_authid".to_string();
        assert_eq!(validate(&forged), Err(StatisticsError::Bound));
    }

    #[test]
    fn reltuples_rounding_is_not_an_exact_claim() {
        assert_eq!(classify_reltuples(-1.0), Err(StatisticsError::EstimateUnknown));
        assert_eq!(classify_reltuples(-2.0), Err(StatisticsError::EstimateUnusable));
        assert_eq!(classify_reltuples(f32::NAN), Err(StatisticsError::EstimateUnusable));
        assert_eq!(classify_reltuples(f32::INFINITY), Err(StatisticsError::EstimateUnusable));
        assert_eq!(classify_reltuples(0.0), Ok(0));
        assert_eq!(classify_reltuples(2.0), Ok(2));
        assert_eq!(classify_reltuples(2.5), Ok(3));
    }
}
