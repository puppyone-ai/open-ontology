//! Live `PolicyCatalog` loader for one trusted TLS session.
//!
//! The caller has already authenticated the PostgreSQL session. This module
//! does not accept a token, an HTTP request, a password, or a claims map.
//! Published classification comes from [`CompiledPolicy`] values produced by
//! [`super::compile_column_acl`]. Those values restrict columns. They do not
//! grant anything. A column is visible only when that classification and
//! `has_column_privilege` both allow it.
//!
//! Function and operator names are the constants below. The loader never
//! resolves a name supplied by the caller. The broader R4 whitelist in the
//! gateway spec stays open; this slice loads only the subset documented in
//! `contracts/gateway/policy-catalog-v1.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, Error as PgError, IsolationLevel, Transaction};

use crate::sql_guard::{
    LiteralTypes, PolicyCatalog, PolicyIdentity, ResolvedFunction, ResolvedOperator, VisibleColumn,
    VisibleRelation,
};

use super::CompiledPolicy;

/// Base types this loader can place on a visible column.
pub const SUPPORTED_TYPES: &[&str] = &["bool", "int2", "int4", "int8", "numeric", "text"];

/// Precision ceiling for a constrained `numeric` column. Matches the builder
/// bound that treats precision above 38 as unbounded or wide.
const NUMERIC_MAX_PRECISION: i32 = 38;

/// `true` when `name` is an allowlisted base type this loader can publish.
///
/// `numeric` also requires `atttypmod >= 0` and precision `1..=38`, using the
/// same high 16 bits as the query decoder. Other allowlisted names ignore
/// `typmod`. The caller still requires `typtype` `b`.
pub fn grantable_base_type(name: &str, typmod: i32) -> bool {
    if !SUPPORTED_TYPES.contains(&name) {
        return false;
    }
    if name != "numeric" {
        return true;
    }
    match numeric_grant_precision(typmod) {
        Some(precision) => (1..=NUMERIC_MAX_PRECISION).contains(&precision),
        None => false,
    }
}

/// Precision of a constrained numeric typmod. `None` means unconstrained.
fn numeric_grant_precision(typmod: i32) -> Option<i32> {
    if typmod < 0 {
        return None;
    }
    let adjusted = typmod.wrapping_sub(4);
    Some((adjusted >> 16) & 0xFFFF)
}

/// Comparison and equality tokens. Arithmetic and pattern operators are absent.
pub const COMPARISON_OPERATORS: &[&str] = &["<", "<=", "<>", "=", ">", ">="];

/// Same-type pairs, plus the int4 pairs an integer literal needs on either side.
///
/// Integer literals use the `int4` type OID. `int2` and `int8` columns compare
/// with that literal only through the cross-type builtin operators.
pub const COMPARISON_SIGNATURES: &[(&str, &str)] = &[
    ("bool", "bool"),
    ("int2", "int2"),
    ("int2", "int4"),
    ("int4", "int2"),
    ("int4", "int4"),
    ("int4", "int8"),
    ("int8", "int4"),
    ("int8", "int8"),
    ("text", "text"),
];

/// The only function this loader resolves: `pg_catalog.count(*)`.
pub const AGGREGATE_STAR_NAME: &str = "count";

/// Type OID used for integer literals. Not a hard-coded PostgreSQL OID.
pub const INTEGER_LITERAL_TYPE: &str = "int4";

/// Type OID used for text literals.
pub const TEXT_LITERAL_TYPE: &str = "text";

/// Type OID used for boolean literals.
pub const BOOLEAN_LITERAL_TYPE: &str = "bool";

const PHYSICAL_SCHEMA: &str = "_oo";
const ABS_MAX_TABLES: u32 = 32;
const ABS_MAX_COLUMNS: u32 = 512;
const ABS_MAX_FUNCTIONS: u32 = 8;
const ABS_MAX_OPERATORS: u32 = 128;

/// What to store when `pg_class.reltuples` is the unknown sentinel `-1`.
///
/// The caller chooses. The loader does not pick a hidden default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnknownRows {
    /// Refuse the load. No row count is invented.
    Refuse,
    /// Store `0`. An unfiltered star is not treated as a large table.
    Zero,
    /// Store [`u64::MAX`]. An unfiltered star fails the guard's row threshold.
    Saturate,
}

/// Bounds checked before catalog rows are collected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogLimits {
    /// Maximum compiled tables in one load.
    pub max_tables: u32,
    /// Maximum physical columns read from one mapped table.
    pub max_columns: u32,
    /// Maximum function rows. Must be at least the one required aggregate.
    pub max_functions: u32,
    /// Maximum operator rows. Must be at least the required comparison set.
    pub max_operators: u32,
    /// Copied onto [`PolicyCatalog::large_table_row_threshold`].
    pub large_table_row_threshold: u64,
    /// Applied only when `reltuples` is `-1`.
    pub unknown_rows: UnknownRows,
}

/// Loader refusal. Display text has no SQL, no DSN, and no hidden column name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogError {
    /// No compiled policy was supplied.
    EmptyInput,
    /// The identity text is unusable, or it disagrees with the policies.
    IdentityMismatch,
    /// The policies do not share one role, project, digest, generation, and login.
    MixedIdentity,
    /// A limit is outside the accepted range.
    InvalidLimit,
    /// The digest and generation are not the stored maximum.
    StaleGeneration,
    /// Lookup did not return the mapped object.
    MappingAbsent,
    /// More than one stored mapping matched.
    AmbiguousMapping,
    /// `pg_roles` has no row for the identity role.
    MissingRole,
    /// The role is not the `NOLOGIN` shape the column installer creates.
    RoleMismatch,
    /// A published column's type is not one supported base type.
    UnsupportedType,
    /// A required type, function, or operator OID is missing.
    MissingObject,
    /// A required type, function, or operator matched more than once.
    AmbiguousObject,
    /// A count exceeded a caller limit before the rows were collected.
    LimitExceeded,
    /// `reltuples` is unknown and the caller selected [`UnknownRows::Refuse`].
    UnknownRows,
    /// `reltuples` is not a finite, usable estimate.
    EstimateUnusable,
    /// `pg_stat_ssl.ssl` is not true for this backend.
    TlsRequired,
    /// The repeatable-read read-only snapshot was not established.
    Snapshot,
    /// Commit or rollback did not finish, or the session did not accept a new statement.
    TransactionIncomplete,
    /// PostgreSQL rejected a statement. The driver message is discarded.
    Database {
        /// SQLSTATE, or empty when the driver has none.
        sqlstate: String,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput => formatter.write_str("policy catalog input is empty"),
            Self::IdentityMismatch => formatter.write_str("policy identity is not coherent"),
            Self::MixedIdentity => formatter.write_str("policy catalog mixes identities"),
            Self::InvalidLimit => formatter.write_str("policy catalog limit is not valid"),
            Self::StaleGeneration => formatter.write_str("policy generation is not current"),
            Self::MappingAbsent => formatter.write_str("stored physical mapping is absent"),
            Self::AmbiguousMapping => formatter.write_str("stored physical mapping is ambiguous"),
            Self::MissingRole => formatter.write_str("policy role is absent"),
            Self::RoleMismatch => formatter.write_str("policy role attributes do not match"),
            Self::UnsupportedType => formatter.write_str("column type is not supported"),
            Self::MissingObject => formatter.write_str("required catalog object is absent"),
            Self::AmbiguousObject => formatter.write_str("required catalog object is ambiguous"),
            Self::LimitExceeded => formatter.write_str("policy catalog limit was exceeded"),
            Self::UnknownRows => formatter.write_str("row estimate is unknown"),
            Self::EstimateUnusable => formatter.write_str("row estimate is not usable"),
            Self::TlsRequired => formatter.write_str("policy catalog requires a TLS session"),
            Self::Snapshot => formatter.write_str("policy catalog snapshot was not established"),
            Self::TransactionIncomplete => {
                formatter.write_str("policy catalog transaction did not complete")
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

impl std::error::Error for CatalogError {}

struct BoundInput {
    project: String,
    role: String,
    config_digest: String,
    generation: i64,
    tables: Vec<TableBound>,
}

struct TableBound {
    namespace: String,
    name: String,
    columns: Vec<String>,
}

struct Attribute {
    attnum: i16,
    name: String,
    type_oid: u32,
    typmod: i32,
    type_name: String,
    type_kind: String,
}

/// Load one [`PolicyCatalog`] from the current mapping, grants, and `pg_catalog`.
///
/// `client` must already be a TLS session. The function opens one
/// `REPEATABLE READ READ ONLY` transaction, checks that it completed, and
/// does not select rows from a mapped table.
pub async fn load_policy_catalog(
    client: &mut Client,
    identity: &PolicyIdentity,
    policies: &[CompiledPolicy],
    limits: &CatalogLimits,
) -> Result<PolicyCatalog, CatalogError> {
    let bound = prepare(identity, policies, limits)?;
    require_tls(client).await?;
    let tx = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .map_err(db_error)?;
    match load_in_snapshot(&tx, identity, &bound, limits).await {
        Ok(catalog) => {
            // Commit drops the transaction borrow before the idle check.
            commit_snapshot(tx).await?;
            ensure_committed(client).await?;
            Ok(catalog)
        }
        Err(err) => match tx.rollback().await {
            Ok(()) => Err(err),
            Err(_) => Err(CatalogError::TransactionIncomplete),
        },
    }
}

fn prepare(
    identity: &PolicyIdentity,
    policies: &[CompiledPolicy],
    limits: &CatalogLimits,
) -> Result<BoundInput, CatalogError> {
    if policies.is_empty() {
        return Err(CatalogError::EmptyInput);
    }
    check_limits(limits, policies.len())?;
    if !identity_text_ok(identity) {
        return Err(CatalogError::IdentityMismatch);
    }
    let first = &policies[0];
    if identity.project != first.project
        || identity.role != first.role
        || identity.policy_id != first.config_digest
    {
        return Err(CatalogError::IdentityMismatch);
    }
    let mut seen = BTreeSet::new();
    let mut tables = Vec::with_capacity(policies.len());
    for policy in policies {
        if policy.project != first.project
            || policy.role != first.role
            || policy.config_digest != first.config_digest
            || policy.generation != first.generation
            || policy.login != first.login
        {
            return Err(CatalogError::MixedIdentity);
        }
        if policy.columns.is_empty() {
            return Err(CatalogError::EmptyInput);
        }
        if !seen.insert((policy.namespace.clone(), policy.table.clone())) {
            return Err(CatalogError::MixedIdentity);
        }
        tables.push(TableBound {
            namespace: policy.namespace.clone(),
            name: policy.table.clone(),
            columns: policy
                .columns
                .iter()
                .map(|column| column.name.clone())
                .collect(),
        });
    }
    Ok(BoundInput {
        project: first.project.clone(),
        role: first.role.clone(),
        config_digest: first.config_digest.clone(),
        generation: first.generation,
        tables,
    })
}

fn check_limits(limits: &CatalogLimits, tables: usize) -> Result<(), CatalogError> {
    let Some(operators) = u32::try_from(required_operator_count()).ok() else {
        return Err(CatalogError::InvalidLimit);
    };
    if limits.max_tables == 0 || limits.max_tables > ABS_MAX_TABLES {
        return Err(CatalogError::InvalidLimit);
    }
    if tables > usize::try_from(limits.max_tables).unwrap_or(0) {
        return Err(CatalogError::LimitExceeded);
    }
    if limits.max_columns == 0 || limits.max_columns > ABS_MAX_COLUMNS {
        return Err(CatalogError::InvalidLimit);
    }
    if limits.max_functions < 1 || limits.max_functions > ABS_MAX_FUNCTIONS {
        return Err(CatalogError::InvalidLimit);
    }
    if limits.max_operators < operators || limits.max_operators > ABS_MAX_OPERATORS {
        return Err(CatalogError::InvalidLimit);
    }
    if !whitelist_tokens_ok() {
        return Err(CatalogError::MissingObject);
    }
    Ok(())
}

fn required_operator_count() -> usize {
    COMPARISON_OPERATORS.len() * COMPARISON_SIGNATURES.len()
}

fn whitelist_tokens_ok() -> bool {
    SUPPORTED_TYPES.iter().all(|name| type_name_ok(name))
        && COMPARISON_SIGNATURES
            .iter()
            .all(|(left, right)| type_name_ok(left) && type_name_ok(right))
        && COMPARISON_OPERATORS
            .iter()
            .all(|name| operator_name_ok(name))
        && type_name_ok(AGGREGATE_STAR_NAME)
        && type_name_ok(INTEGER_LITERAL_TYPE)
        && type_name_ok(TEXT_LITERAL_TYPE)
        && type_name_ok(BOOLEAN_LITERAL_TYPE)
}

fn type_name_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn operator_name_ok(value: &str) -> bool {
    matches!(value, "<" | "<=" | "<>" | "=" | ">" | ">=")
}

fn identity_text_ok(identity: &PolicyIdentity) -> bool {
    closed_identity(&identity.project)
        && closed_identity(&identity.role)
        && closed_identity(&identity.policy_id)
}

fn closed_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.chars().any(|char| char == '\0' || char.is_control())
}

async fn require_tls(client: &Client) -> Result<(), CatalogError> {
    let row = client
        .query_one(
            "SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()",
            &[],
        )
        .await
        .map_err(db_error)?;
    let ssl: bool = row.get(0);
    if ssl {
        Ok(())
    } else {
        Err(CatalogError::TlsRequired)
    }
}

async fn load_in_snapshot(
    tx: &Transaction<'_>,
    identity: &PolicyIdentity,
    bound: &BoundInput,
    limits: &CatalogLimits,
) -> Result<PolicyCatalog, CatalogError> {
    tx.batch_execute("SET LOCAL search_path TO pg_catalog; SET LOCAL statement_timeout = '30s'")
        .await
        .map_err(db_error)?;
    ensure_snapshot(tx).await?;
    ensure_generation(tx, bound).await?;
    ensure_role(tx, &bound.role).await?;
    let types = load_types(tx).await?;
    let operators = load_operators(tx, &types, limits).await?;
    let functions = load_count(tx, &types, limits).await?;
    let mut relations = Vec::with_capacity(bound.tables.len());
    for table in &bound.tables {
        if let Some(relation) = load_relation(tx, bound, table, &types, limits).await? {
            relations.push(relation);
        }
    }
    ensure_snapshot(tx).await?;
    let integer = *types
        .get(INTEGER_LITERAL_TYPE)
        .ok_or(CatalogError::MissingObject)?;
    let text = *types
        .get(TEXT_LITERAL_TYPE)
        .ok_or(CatalogError::MissingObject)?;
    let boolean = *types
        .get(BOOLEAN_LITERAL_TYPE)
        .ok_or(CatalogError::MissingObject)?;
    Ok(PolicyCatalog {
        identity: identity.clone(),
        relations,
        functions,
        operators,
        literal_types: LiteralTypes {
            integer: Some(integer),
            text: Some(text),
            boolean: Some(boolean),
        },
        null_test_allowed: true,
        large_table_row_threshold: limits.large_table_row_threshold,
    })
}

async fn ensure_snapshot(tx: &Transaction<'_>) -> Result<(), CatalogError> {
    let row = tx
        .query_one(
            "SELECT current_setting('transaction_isolation'), \
                    current_setting('transaction_read_only'), \
                    current_setting('search_path'), \
                    current_setting('statement_timeout')",
            &[],
        )
        .await
        .map_err(db_error)?;
    let isolation: String = row.get(0);
    let read_only: String = row.get(1);
    let search_path: String = row.get(2);
    let timeout: String = row.get(3);
    if isolation == "repeatable read"
        && read_only == "on"
        && (search_path == "pg_catalog" || search_path == "\"pg_catalog\"")
        && matches!(timeout.as_str(), "30s" | "30000" | "30000ms")
    {
        Ok(())
    } else {
        Err(CatalogError::Snapshot)
    }
}

async fn ensure_generation(tx: &Transaction<'_>, bound: &BoundInput) -> Result<(), CatalogError> {
    let row = tx
        .query_one(
            "SELECT max(run_generation) FROM _oo.physical_mappings \
             WHERE source = $1 AND config_digest = $2",
            &[&bound.project, &bound.config_digest],
        )
        .await
        .map_err(db_error)?;
    let current: Option<i64> = row.get(0);
    if current == Some(bound.generation) {
        Ok(())
    } else {
        Err(CatalogError::StaleGeneration)
    }
}

async fn ensure_role(tx: &Transaction<'_>, role: &str) -> Result<(), CatalogError> {
    let rows = tx
        .query(
            "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, \
                    rolbypassrls, rolreplication \
             FROM pg_catalog.pg_roles WHERE rolname = $1",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    if rows.is_empty() {
        return Err(CatalogError::MissingRole);
    }
    if rows.len() != 1 {
        return Err(CatalogError::AmbiguousObject);
    }
    let flags: [bool; 7] = [
        rows[0].get(0),
        rows[0].get(1),
        rows[0].get(2),
        rows[0].get(3),
        rows[0].get(4),
        rows[0].get(5),
        rows[0].get(6),
    ];
    if flags.iter().any(|flag| *flag) {
        Err(CatalogError::RoleMismatch)
    } else {
        Ok(())
    }
}

async fn load_types(tx: &Transaction<'_>) -> Result<BTreeMap<String, u32>, CatalogError> {
    let names: Vec<String> = SUPPORTED_TYPES
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    let expected = i64::try_from(names.len()).map_err(|_| CatalogError::LimitExceeded)?;
    let count_row = tx
        .query_one(
            "SELECT count(*)::int8 \
             FROM pg_catalog.pg_type AS type_row \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type_row.typnamespace \
             WHERE namespace.nspname = 'pg_catalog' \
               AND type_row.typtype = 'b' \
               AND type_row.typname = ANY($1::text[])",
            &[&names],
        )
        .await
        .map_err(db_error)?;
    let count: i64 = count_row.get(0);
    if count != expected {
        return if count > expected {
            Err(CatalogError::AmbiguousObject)
        } else {
            Err(CatalogError::MissingObject)
        };
    }
    let rows = tx
        .query(
            "SELECT type_row.typname, type_row.oid::int4 \
             FROM pg_catalog.pg_type AS type_row \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type_row.typnamespace \
             WHERE namespace.nspname = 'pg_catalog' \
               AND type_row.typtype = 'b' \
               AND type_row.typname = ANY($1::text[])",
            &[&names],
        )
        .await
        .map_err(db_error)?;
    if i64::try_from(rows.len()).ok() != Some(expected) {
        return Err(CatalogError::Snapshot);
    }
    let mut types = BTreeMap::new();
    for row in rows {
        let name: String = row.get(0);
        let oid = oid_from_i32(row.get(1))?;
        if !names.iter().any(|wanted| wanted == &name) || types.insert(name, oid).is_some() {
            return Err(CatalogError::AmbiguousObject);
        }
    }
    if types.len() != names.len() {
        return Err(CatalogError::MissingObject);
    }
    Ok(types)
}

async fn load_operators(
    tx: &Transaction<'_>,
    types: &BTreeMap<String, u32>,
    limits: &CatalogLimits,
) -> Result<Vec<ResolvedOperator>, CatalogError> {
    let bool_oid = types
        .get(BOOLEAN_LITERAL_TYPE)
        .ok_or(CatalogError::MissingObject)?;
    let bool_i = i32::try_from(*bool_oid).map_err(|_| CatalogError::MissingObject)?;
    let (names, lefts, rights) = wanted_operators();
    let expected = i64::try_from(names.len()).map_err(|_| CatalogError::LimitExceeded)?;
    if expected > i64::from(limits.max_operators) {
        return Err(CatalogError::LimitExceeded);
    }
    let params: [&(dyn ToSql + Sync); 4] = [&names, &lefts, &rights, &bool_i];
    let count_row = tx
        .query_one(
            &format!("SELECT count(*)::int8, count(DISTINCT operator.oid)::int8 {OPERATOR_FROM}"),
            &params,
        )
        .await
        .map_err(db_error)?;
    let count: i64 = count_row.get(0);
    let distinct: i64 = count_row.get(1);
    if count != expected || distinct != expected {
        return if count > expected || distinct != count {
            Err(CatalogError::AmbiguousObject)
        } else {
            Err(CatalogError::MissingObject)
        };
    }
    let rows = tx
        .query(
            &format!(
                "SELECT operator.oid::int4, operator.oprcode::oid::int4, operator.oprname, \
                        left_type.typname, right_type.typname {OPERATOR_FROM} \
                 ORDER BY operator.oprname, left_type.typname, right_type.typname"
            ),
            &params,
        )
        .await
        .map_err(db_error)?;
    if i64::try_from(rows.len()).ok() != Some(expected) {
        return Err(CatalogError::Snapshot);
    }
    let mut wanted = BTreeSet::new();
    for index in 0..names.len() {
        wanted.insert((
            names[index].clone(),
            lefts[index].clone(),
            rights[index].clone(),
        ));
    }
    let mut operators = Vec::with_capacity(rows.len());
    for row in rows {
        let name: String = row.get(2);
        let left_name: String = row.get(3);
        let right_name: String = row.get(4);
        if !wanted.remove(&(name.clone(), left_name.clone(), right_name.clone())) {
            return Err(CatalogError::AmbiguousObject);
        }
        let left = *types.get(&left_name).ok_or(CatalogError::MissingObject)?;
        let right = *types.get(&right_name).ok_or(CatalogError::MissingObject)?;
        operators.push(ResolvedOperator {
            oid: oid_from_i32(row.get(0))?,
            procedure_oid: oid_from_i32(row.get(1))?,
            schema: "pg_catalog".to_string(),
            name,
            left_type_oid: left,
            right_type_oid: right,
        });
    }
    if !wanted.is_empty() {
        return Err(CatalogError::MissingObject);
    }
    Ok(operators)
}

fn wanted_operators() -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut names = Vec::new();
    let mut lefts = Vec::new();
    let mut rights = Vec::new();
    for (left, right) in COMPARISON_SIGNATURES {
        for operator in COMPARISON_OPERATORS {
            names.push((*operator).to_string());
            lefts.push((*left).to_string());
            rights.push((*right).to_string());
        }
    }
    (names, lefts, rights)
}

const OPERATOR_FROM: &str = "\
FROM unnest($1::text[], $2::text[], $3::text[]) AS wanted(oprname, left_name, right_name) \
JOIN pg_catalog.pg_operator AS operator \
  ON operator.oprname = wanted.oprname \
 AND operator.oprkind = 'b' \
 AND operator.oprresult = ($4::int4)::oid \
JOIN pg_catalog.pg_namespace AS namespace \
  ON namespace.oid = operator.oprnamespace \
 AND namespace.nspname = 'pg_catalog' \
JOIN pg_catalog.pg_type AS left_type \
  ON left_type.oid = operator.oprleft \
 AND left_type.typname = wanted.left_name \
 AND left_type.typtype = 'b' \
JOIN pg_catalog.pg_namespace AS left_namespace \
  ON left_namespace.oid = left_type.typnamespace \
 AND left_namespace.nspname = 'pg_catalog' \
JOIN pg_catalog.pg_type AS right_type \
  ON right_type.oid = operator.oprright \
 AND right_type.typname = wanted.right_name \
 AND right_type.typtype = 'b' \
JOIN pg_catalog.pg_namespace AS right_namespace \
  ON right_namespace.oid = right_type.typnamespace \
 AND right_namespace.nspname = 'pg_catalog' \
JOIN pg_catalog.pg_proc AS procedure \
  ON procedure.oid = operator.oprcode \
 AND procedure.prokind = 'f' \
 AND procedure.provolatile = 'i' \
 AND procedure.pronargs = 2 \
 AND NOT procedure.proretset \
 AND procedure.prorettype = ($4::int4)::oid \
 AND procedure.proargtypes[0] = operator.oprleft \
 AND procedure.proargtypes[1] = operator.oprright \
JOIN pg_catalog.pg_language AS language \
  ON language.oid = procedure.prolang \
 AND language.lanname = 'internal' \
WHERE NOT EXISTS ( \
    SELECT 1 FROM pg_catalog.pg_depend AS depend \
    WHERE depend.objid = operator.oid \
      AND depend.classid = 'pg_catalog.pg_operator'::regclass \
      AND depend.deptype = 'e' \
  ) \
  AND NOT EXISTS ( \
    SELECT 1 FROM pg_catalog.pg_depend AS depend \
    WHERE depend.objid = procedure.oid \
      AND depend.classid = 'pg_catalog.pg_proc'::regclass \
      AND depend.deptype = 'e' \
  )";

async fn load_count(
    tx: &Transaction<'_>,
    types: &BTreeMap<String, u32>,
    limits: &CatalogLimits,
) -> Result<Vec<ResolvedFunction>, CatalogError> {
    if limits.max_functions < 1 {
        return Err(CatalogError::LimitExceeded);
    }
    let int8_oid = types.get("int8").ok_or(CatalogError::MissingObject)?;
    let int8_i = i32::try_from(*int8_oid).map_err(|_| CatalogError::MissingObject)?;
    let name = AGGREGATE_STAR_NAME.to_string();
    let params: [&(dyn ToSql + Sync); 2] = [&name, &int8_i];
    let count_row = tx
        .query_one(&format!("SELECT count(*)::int8 {COUNT_FROM}"), &params)
        .await
        .map_err(db_error)?;
    let count: i64 = count_row.get(0);
    if count != 1 {
        return if count > 1 {
            Err(CatalogError::AmbiguousObject)
        } else {
            Err(CatalogError::MissingObject)
        };
    }
    let row = tx
        .query_one(
            &format!("SELECT procedure.oid::int4, procedure.proname {COUNT_FROM}"),
            &params,
        )
        .await
        .map_err(db_error)?;
    let function_name: String = row.get(1);
    if function_name != AGGREGATE_STAR_NAME {
        return Err(CatalogError::MissingObject);
    }
    Ok(vec![ResolvedFunction {
        oid: oid_from_i32(row.get(0))?,
        schema: "pg_catalog".to_string(),
        name: function_name,
        argument_type_oids: Vec::new(),
        aggregate_star: true,
    }])
}

const COUNT_FROM: &str = "\
FROM pg_catalog.pg_proc AS procedure \
JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = procedure.pronamespace \
JOIN pg_catalog.pg_aggregate AS aggregate ON aggregate.aggfnoid = procedure.oid \
JOIN pg_catalog.pg_language AS language ON language.oid = procedure.prolang \
WHERE namespace.nspname = 'pg_catalog' \
  AND procedure.proname = $1 \
  AND procedure.prokind = 'a' \
  AND procedure.pronargs = 0 \
  AND procedure.provolatile = 'i' \
  AND NOT procedure.proretset \
  AND procedure.prorettype = ($2::int4)::oid \
  AND aggregate.aggkind = 'n' \
  AND aggregate.aggnumdirectargs = 0 \
  AND language.lanname = 'internal' \
  AND NOT EXISTS ( \
      SELECT 1 FROM pg_catalog.pg_depend AS depend \
      WHERE depend.objid = procedure.oid \
        AND depend.classid = 'pg_catalog.pg_proc'::regclass \
        AND depend.deptype = 'e' \
    )";

async fn load_relation(
    tx: &Transaction<'_>,
    bound: &BoundInput,
    table: &TableBound,
    types: &BTreeMap<String, u32>,
    limits: &CatalogLimits,
) -> Result<Option<VisibleRelation>, CatalogError> {
    let physical_table =
        mapped_name(tx, bound, LEVEL_TABLE, &table.namespace, &table.name, "").await?;
    let mut published = Vec::with_capacity(table.columns.len());
    for column in &table.columns {
        let physical = mapped_name(
            tx,
            bound,
            LEVEL_COLUMN,
            &table.namespace,
            &table.name,
            column,
        )
        .await?;
        published.push((column.clone(), physical));
    }
    let relation = relation_row(tx, &physical_table).await?;
    let attribute_count = count_attributes(tx, relation.oid).await?;
    if attribute_count > i64::from(limits.max_columns) {
        return Err(CatalogError::LimitExceeded);
    }
    let attributes = fetch_attributes(tx, relation.oid).await?;
    if i64::try_from(attributes.len()).ok() != Some(attribute_count) {
        return Err(CatalogError::Snapshot);
    }
    let mut by_name = BTreeMap::new();
    for attribute in &attributes {
        if by_name.insert(attribute.name.clone(), attribute).is_some() {
            return Err(CatalogError::AmbiguousMapping);
        }
    }
    let mut visible = Vec::new();
    for (logical, physical) in &published {
        let attribute = by_name.get(physical).ok_or(CatalogError::MappingAbsent)?;
        if !supported_base(attribute, types) {
            return Err(CatalogError::UnsupportedType);
        }
        if column_select(tx, &bound.role, relation.oid, attribute.attnum).await? {
            visible.push((
                attribute.attnum,
                VisibleColumn {
                    name: logical.clone(),
                    physical_name: physical.clone(),
                    type_oid: attribute.type_oid,
                },
            ));
        }
    }
    visible.sort_by_key(|(attnum, _)| *attnum);
    if visible.is_empty() {
        return Ok(None);
    }
    let pending =
        u32::try_from(attributes.len() - visible.len()).map_err(|_| CatalogError::LimitExceeded)?;
    let columns = visible.into_iter().map(|(_, column)| column).collect();
    Ok(Some(VisibleRelation {
        schema: table.namespace.clone(),
        name: table.name.clone(),
        physical_schema: PHYSICAL_SCHEMA.to_string(),
        physical_name: physical_table,
        columns,
        pending_unclassified: pending,
        estimated_rows: estimate_rows(relation.reltuples, limits.unknown_rows)?,
    }))
}

struct RelationRow {
    oid: i32,
    reltuples: f32,
}

async fn relation_row(tx: &Transaction<'_>, physical: &str) -> Result<RelationRow, CatalogError> {
    let count_row = tx
        .query_one(
            "SELECT count(*)::int8 \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = $1 AND relation.relname = $2 AND relation.relkind = 'r'",
            &[&PHYSICAL_SCHEMA, &physical],
        )
        .await
        .map_err(db_error)?;
    let count: i64 = count_row.get(0);
    if count != 1 {
        return if count == 0 {
            Err(CatalogError::MappingAbsent)
        } else {
            Err(CatalogError::AmbiguousMapping)
        };
    }
    let row = tx
        .query_one(
            "SELECT relation.oid::int4, relation.reltuples \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = $1 AND relation.relname = $2 AND relation.relkind = 'r'",
            &[&PHYSICAL_SCHEMA, &physical],
        )
        .await
        .map_err(db_error)?;
    Ok(RelationRow {
        oid: row.get(0),
        reltuples: row.get(1),
    })
}

async fn count_attributes(tx: &Transaction<'_>, relation: i32) -> Result<i64, CatalogError> {
    let row = tx
        .query_one(
            "SELECT count(*)::int8 FROM pg_catalog.pg_attribute AS attribute \
             WHERE attribute.attrelid = ($1::int4)::oid \
               AND attribute.attnum > 0 \
               AND NOT attribute.attisdropped",
            &[&relation],
        )
        .await
        .map_err(db_error)?;
    Ok(row.get(0))
}

async fn fetch_attributes(
    tx: &Transaction<'_>,
    relation: i32,
) -> Result<Vec<Attribute>, CatalogError> {
    let rows = tx
        .query(
            "SELECT attribute.attnum, attribute.attname::text, attribute.atttypid::int4, \
                    attribute.atttypmod::int4, type_row.typname::text, type_row.typtype::text \
             FROM pg_catalog.pg_attribute AS attribute \
             JOIN pg_catalog.pg_type AS type_row ON type_row.oid = attribute.atttypid \
             WHERE attribute.attrelid = ($1::int4)::oid \
               AND attribute.attnum > 0 \
               AND NOT attribute.attisdropped",
            &[&relation],
        )
        .await
        .map_err(db_error)?;
    let mut attributes = Vec::with_capacity(rows.len());
    for row in rows {
        attributes.push(Attribute {
            attnum: row.get(0),
            name: row.get(1),
            type_oid: oid_from_i32(row.get(2))?,
            typmod: row.get(3),
            type_name: row.get(4),
            type_kind: row.get(5),
        });
    }
    Ok(attributes)
}

fn supported_base(attribute: &Attribute, types: &BTreeMap<String, u32>) -> bool {
    if attribute.type_kind != "b" {
        return false;
    }
    match types.get(&attribute.type_name) {
        Some(oid) => {
            *oid == attribute.type_oid
                && grantable_base_type(&attribute.type_name, attribute.typmod)
        }
        None => false,
    }
}

async fn column_select(
    tx: &Transaction<'_>,
    role: &str,
    relation: i32,
    attnum: i16,
) -> Result<bool, CatalogError> {
    let row = tx
        .query_one(
            "SELECT pg_catalog.has_column_privilege(($1::text)::name, ($2::int4)::oid, $3::int2, 'SELECT')",
            &[&role, &relation, &attnum],
        )
        .await
        .map_err(db_error)?;
    let allowed: Option<bool> = row.get(0);
    Ok(allowed == Some(true))
}

struct MapLevel {
    word: &'static str,
    letter: &'static str,
    prefix: &'static str,
}

const LEVEL_TABLE: MapLevel = MapLevel {
    word: "table",
    letter: "t",
    prefix: "pt_",
};
const LEVEL_COLUMN: MapLevel = MapLevel {
    word: "column",
    letter: "c",
    prefix: "pc_",
};

async fn mapped_name(
    tx: &Transaction<'_>,
    bound: &BoundInput,
    level: MapLevel,
    namespace: &str,
    table: &str,
    column: &str,
) -> Result<String, CatalogError> {
    let count_row = tx
        .query_one(
            "SELECT count(*)::int8 FROM _oo.physical_mappings \
             WHERE source = $1 AND config_digest = $2 AND run_generation = $3 \
               AND level = $4 AND logical_namespace = $5 AND logical_table = $6 \
               AND logical_column = $7",
            &[
                &bound.project,
                &bound.config_digest,
                &bound.generation,
                &level.letter,
                &namespace,
                &table,
                &column,
            ],
        )
        .await
        .map_err(db_error)?;
    let count: i64 = count_row.get(0);
    if count != 1 {
        return if count == 0 {
            Err(CatalogError::MappingAbsent)
        } else {
            Err(CatalogError::AmbiguousMapping)
        };
    }
    let stored_row = tx
        .query_one(
            "SELECT physical_name FROM _oo.physical_mappings \
             WHERE source = $1 AND config_digest = $2 AND run_generation = $3 \
               AND level = $4 AND logical_namespace = $5 AND logical_table = $6 \
               AND logical_column = $7",
            &[
                &bound.project,
                &bound.config_digest,
                &bound.generation,
                &level.letter,
                &namespace,
                &table,
                &column,
            ],
        )
        .await
        .map_err(db_error)?;
    let stored: String = stored_row.get(0);
    let looked_row = tx
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &[
                &bound.project,
                &bound.config_digest,
                &bound.generation,
                &level.word,
                &namespace,
                &table,
                &column,
            ],
        )
        .await
        .map_err(db_error)?;
    let looked: String = looked_row.get(0);
    if stored != looked || !physical_ok(&stored, level.prefix) {
        return Err(CatalogError::AmbiguousMapping);
    }
    Ok(stored)
}

fn physical_ok(value: &str, prefix: &str) -> bool {
    let Some(rest) = value.strip_prefix(prefix) else {
        return false;
    };
    value.len() == 63
        && rest.len() == 60
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn estimate_rows(value: f32, policy: UnknownRows) -> Result<u64, CatalogError> {
    if !value.is_finite() {
        return Err(CatalogError::EstimateUnusable);
    }
    if value < 0.0 {
        return if value == -1.0 {
            match policy {
                UnknownRows::Refuse => Err(CatalogError::UnknownRows),
                UnknownRows::Zero => Ok(0),
                UnknownRows::Saturate => Ok(u64::MAX),
            }
        } else {
            Err(CatalogError::EstimateUnusable)
        };
    }
    let rounded = f64::from(value).round();
    if !(0.0..18_446_744_073_709_551_616.0).contains(&rounded) {
        return Err(CatalogError::EstimateUnusable);
    }
    Ok(rounded as u64)
}

fn oid_from_i32(value: i32) -> Result<u32, CatalogError> {
    u32::try_from(value).map_err(|_| CatalogError::MissingObject)
}

async fn commit_snapshot(tx: Transaction<'_>) -> Result<(), CatalogError> {
    tx.commit()
        .await
        .map_err(|_| CatalogError::TransactionIncomplete)
}

async fn ensure_committed(client: &Client) -> Result<(), CatalogError> {
    let row = client
        .query_one(
            "SELECT pg_catalog.pg_current_xact_id_if_assigned() IS NULL",
            &[],
        )
        .await
        .map_err(|_| CatalogError::TransactionIncomplete)?;
    let idle: bool = row.get(0);
    if idle {
        Ok(())
    } else {
        Err(CatalogError::TransactionIncomplete)
    }
}

fn db_error(err: PgError) -> CatalogError {
    let sqlstate = err
        .code()
        .map(|code| code.code().to_string())
        .unwrap_or_default();
    if sqlstate == "OO024" {
        CatalogError::MappingAbsent
    } else {
        CatalogError::Database { sqlstate }
    }
}

fn sqlstate_ok(value: &str) -> bool {
    value.len() == 5 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::super::{
        ClassStatus, ColumnAclPolicy, ColumnSpec, LogicalTable, PiiClass, compile_column_acl,
    };
    use super::*;

    fn column(name: &str) -> ColumnSpec {
        ColumnSpec {
            name: name.to_string(),
            status: ClassStatus::Confirmed,
            semantic_type: "text".to_string(),
            pii: PiiClass::None,
        }
    }

    fn policy(role: &str, columns: Vec<&str>) -> CompiledPolicy {
        compile_column_acl(&ColumnAclPolicy {
            project: "ledger".to_string(),
            config_digest: "ab".repeat(32),
            generation: 11,
            role: role.to_string(),
            login: "oo_a190_login".to_string(),
            table: LogicalTable {
                namespace: "billing".to_string(),
                name: "invoice".to_string(),
                columns: columns.into_iter().map(column).collect(),
            },
            row_filter: None,
        })
        .expect("compile")
    }

    fn limits() -> CatalogLimits {
        CatalogLimits {
            max_tables: 4,
            max_columns: 64,
            max_functions: 1,
            max_operators: u32::try_from(required_operator_count()).expect("operator count"),
            large_table_row_threshold: 1_000_000,
            unknown_rows: UnknownRows::Refuse,
        }
    }

    fn refused(result: Result<BoundInput, CatalogError>) -> CatalogError {
        match result {
            Err(err) => err,
            Ok(_) => panic!("prepare returned a bound input"),
        }
    }

    fn identity() -> PolicyIdentity {
        PolicyIdentity {
            project: "ledger".to_string(),
            role: "oo_r_clerk".to_string(),
            policy_id: "ab".repeat(32),
        }
    }

    #[test]
    fn bounded_numeric_admits_precision_1_through_38() {
        fn typmod(precision: i32, scale: i32) -> i32 {
            ((precision << 16) | (scale & 0x7FF)) + 4
        }
        assert_eq!(typmod(18, 4), 1_179_656);
        assert!(grantable_base_type("numeric", typmod(18, 4)));
        assert!(grantable_base_type("numeric", typmod(38, 4)));
        assert!(grantable_base_type("numeric", typmod(38, 0)));
        assert!(grantable_base_type("numeric", typmod(1, 0)));
        assert!(grantable_base_type("numeric", typmod(4, 6)));
        assert!(grantable_base_type("numeric", typmod(18, -2)));
        assert!(grantable_base_type("int8", -1));
        assert!(!grantable_base_type("numeric", -1));
        assert!(!grantable_base_type("numeric", typmod(0, 0)));
        assert!(!grantable_base_type("numeric", typmod(39, 4)));
        assert!(!grantable_base_type("numeric", typmod(1000, 0)));
        assert!(!grantable_base_type("float8", typmod(18, 4)));
        assert!(!grantable_base_type("varchar", -1));
        assert_eq!(required_operator_count(), 54);
    }

    #[test]
    fn whitelist_covers_only_the_comparison_subset() {
        assert_eq!(required_operator_count(), 54);
        assert!(COMPARISON_OPERATORS.contains(&"="));
        assert!(COMPARISON_OPERATORS.contains(&"<>"));
        assert!(
            !COMPARISON_OPERATORS
                .iter()
                .any(|name| *name == "+" || *name == "~~")
        );
        assert_eq!(AGGREGATE_STAR_NAME, "count");
        assert!(whitelist_tokens_ok());
    }

    #[test]
    fn unknown_reltuples_follow_the_explicit_policy() {
        assert_eq!(
            estimate_rows(-1.0, UnknownRows::Refuse),
            Err(CatalogError::UnknownRows)
        );
        assert_eq!(estimate_rows(-1.0, UnknownRows::Zero), Ok(0));
        assert_eq!(estimate_rows(-1.0, UnknownRows::Saturate), Ok(u64::MAX));
        assert_eq!(estimate_rows(3.0, UnknownRows::Refuse), Ok(3));
        assert_eq!(estimate_rows(2.5, UnknownRows::Refuse), Ok(3));
        assert_eq!(
            estimate_rows(f32::NAN, UnknownRows::Zero),
            Err(CatalogError::EstimateUnusable)
        );
        assert_eq!(
            estimate_rows(-2.0, UnknownRows::Zero),
            Err(CatalogError::EstimateUnusable)
        );
    }

    #[test]
    fn mixed_and_mismatched_identity_fail_before_a_connection() {
        let clerk = policy("clerk", vec!["amount"]);
        assert_eq!(
            refused(prepare(&identity(), &[], &limits())),
            CatalogError::EmptyInput
        );
        let mut wrong = identity();
        wrong.role = "oo_r_auditor".to_string();
        assert_eq!(
            refused(prepare(&wrong, std::slice::from_ref(&clerk), &limits())),
            CatalogError::IdentityMismatch
        );
        let auditor = policy("auditor", vec!["amount"]);
        assert_eq!(
            refused(prepare(&identity(), &[clerk, auditor], &limits())),
            CatalogError::MixedIdentity
        );
        let mut narrow = limits();
        narrow.max_operators = 1;
        assert_eq!(
            refused(prepare(
                &identity(),
                &[policy("clerk", vec!["amount"])],
                &narrow
            )),
            CatalogError::InvalidLimit
        );
    }

    #[test]
    fn errors_do_not_carry_sql_or_a_hidden_name() {
        let text = CatalogError::UnsupportedType.to_string();
        assert!(!text.contains("SELECT"));
        assert!(!text.contains("secret"));
        let database = CatalogError::Database {
            sqlstate: "42501".to_string(),
        };
        assert_eq!(database.to_string(), "postgres sqlstate=42501");
        let noisy = CatalogError::Database {
            sqlstate: "secret column".to_string(),
        };
        assert_eq!(noisy.to_string(), "postgres error");
    }
}
