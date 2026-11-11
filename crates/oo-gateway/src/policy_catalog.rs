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

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, Error as PgError, IsolationLevel, Transaction};

use crate::sql_guard::{
    LiteralTypes, PolicyCatalog, PolicyIdentity, ResolvedFunction, ResolvedOperator, VisibleColumn,
    VisibleRelation,
};

use super::CompiledPolicy;

#[path = "policy_catalog_snapshot.rs"]
mod snapshot;

#[cfg(test)]
#[path = "policy_catalog_numeric_pg_tests.rs"]
mod numeric_pg_tests;

#[cfg(test)]
#[path = "policy_catalog_capabilities_pg_tests.rs"]
mod capabilities_pg_tests;

/// Base types this loader can place on a visible column.
///
/// The six original names stay. `varchar`, `float8`, `timestamptz`, `json`,
/// `bytea`, `uuid`, `bpchar`, `date`, `timestamp`, `interval`, and `jsonb` are the exact
/// extra base types this loader can publish. `timestamp` is timestamp without
/// time zone. `json` stays oid 114. `float4` and every
/// other name stay out. Publishing a name does not admit a column: the role
/// and the physical mapper still have to allow that column.
pub const SUPPORTED_TYPES: &[&str] = &[
    "bool",
    "int2",
    "int4",
    "int8",
    "numeric",
    "text",
    "varchar",
    "float8",
    "timestamptz",
    "json",
    "bytea",
    "uuid",
    "bpchar",
    "date",
    "time",
    "interval",
    "timestamp",
    "jsonb",
];

/// Decimal256 coefficient ceiling. Precision 1..=38 remains decimal128.
/// Precision 77 and above stays refused.
const NUMERIC_MAX_PRECISION: i32 = 76;

/// `true` when `name` is an allowlisted base type this loader can publish.
///
/// `numeric` requires precision `1..=76`. `uuid`, `date`, `timestamp`, and
/// `jsonb` are legal only at typmod `-1`. A timestamp typmod of `0` or `6`
/// would truncate and stays refused. `bpchar` is legal at typmod `-1`, and at
/// a character typmod whose length `typmod - 4` is `1..=10485760`. Typmod `10`
/// is `character(6)`. `json` stays typmod-ignored. The other allowlisted names
/// ignore typmod. The caller still requires `typtype` `b`.
pub fn grantable_base_type(name: &str, typmod: i32) -> bool {
    if !SUPPORTED_TYPES.contains(&name) {
        return false;
    }
    match name {
        "numeric" => match numeric_grant_precision(typmod) {
            Some(precision) => (1..=NUMERIC_MAX_PRECISION).contains(&precision),
            None => false,
        },
        "uuid" | "date" | "time" | "timestamp" | "interval" | "jsonb" => typmod == -1,
        "bpchar" => character_grant_typmod(typmod),
        _ => true,
    }
}

/// Unconstrained `character`, or `character(n)` with `n` in `1..=10485760`.
fn character_grant_typmod(typmod: i32) -> bool {
    if typmod == -1 {
        return true;
    }
    if typmod < 4 {
        return false;
    }
    (1..=10_485_760).contains(&(typmod - 4))
}

/// Precision of a constrained numeric typmod. `None` means unconstrained.
fn numeric_grant_precision(typmod: i32) -> Option<i32> {
    if typmod < 0 {
        return None;
    }
    let adjusted = typmod.wrapping_sub(4);
    Some((adjusted >> 16) & 0xFFFF)
}

/// Comparison and equality tokens.
pub const COMPARISON_OPERATORS: &[&str] = &["<", "<=", "<>", "=", ">", ">="];

/// LIKE/ILIKE and their negations, only the builtin text/text -> bool overloads.
pub const PATTERN_OPERATORS: &[&str] = &["~~", "!~~", "~~*", "!~~*"];

pub(crate) fn pattern_signature_allowed(operator: &ResolvedOperator) -> bool {
    operator.schema == "pg_catalog"
        && PATTERN_OPERATORS.contains(&operator.name.as_str())
        && operator.left_type_oid == postgres_types::Type::TEXT.oid()
        && operator.right_type_oid == postgres_types::Type::TEXT.oid()
        && operator.result_type_oid == postgres_types::Type::BOOL.oid()
}

/// Binary arithmetic only; no unary, string, temporal, or extension operators.
pub const ARITHMETIC_OPERATORS: &[&str] = &["+", "-", "*", "/"];

/// Exact builtin input and result types. Cross-type pairs are limited to int4.
pub const ARITHMETIC_SIGNATURES: &[(&str, &str, &str)] = &[
    ("int2", "int2", "int2"),
    ("int2", "int4", "int4"),
    ("int4", "int2", "int4"),
    ("int4", "int4", "int4"),
    ("int4", "int8", "int8"),
    ("int8", "int4", "int8"),
    ("int8", "int8", "int8"),
    ("numeric", "numeric", "numeric"),
    ("float8", "float8", "float8"),
];

pub(crate) fn arithmetic_signature_allowed(operator: &ResolvedOperator) -> bool {
    let type_name = |oid| postgres_types::Type::from_oid(oid).map(|kind| kind.name().to_owned());
    operator.schema == "pg_catalog"
        && ARITHMETIC_OPERATORS.contains(&operator.name.as_str())
        && ARITHMETIC_SIGNATURES.iter().any(|(left, right, result)| {
            type_name(operator.left_type_oid).as_deref() == Some(*left)
                && type_name(operator.right_type_oid).as_deref() == Some(*right)
                && type_name(operator.result_type_oid).as_deref() == Some(*result)
        })
}

/// Same-type pairs, plus the int4 pairs an integer literal needs on either side.
///
/// Integer literals use the `int4` type OID. `int2` and `int8` columns compare
/// with that literal only through the cross-type builtin operators. `numeric`
/// columns compare with an integer literal through the `numeric`/`numeric`
/// operator; PostgreSQL casts the literal. A decimal-point literal is still
/// rejected before this list is consulted.
///
/// `date`/`date` is the same-type pair. The loader reads each comparator from
/// pg_catalog and stores `operator.oid` separately from `pg_proc.oid`. Neither
/// OID is hardcoded. Timestamp comparisons are same-type only; no cross-type
/// date/timestamp or timestamp/timestamptz casts are admitted.
pub const COMPARISON_SIGNATURES: &[(&str, &str)] = &[
    ("bool", "bool"),
    ("int2", "int2"),
    ("int2", "int4"),
    ("int4", "int2"),
    ("int4", "int4"),
    ("int4", "int8"),
    ("int8", "int4"),
    ("int8", "int8"),
    ("numeric", "numeric"),
    ("text", "text"),
    ("date", "date"),
    ("timestamp", "timestamp"),
    ("timestamptz", "timestamptz"),
    ("interval", "interval"),
];

/// `pg_catalog.count(*)` and `pg_catalog.count("any")` share this name.
pub const AGGREGATE_STAR_NAME: &str = "count";

/// Exact unary aggregate (name, argument, result) signatures, in budget order.
/// Keep the original prefix stable for callers with smaller explicit budgets.
pub const UNARY_AGGREGATE_SIGNATURES: &[(&str, &str, &str)] = &[
    ("sum", "numeric", "numeric"),
    ("sum", "int8", "numeric"),
    ("min", "int8", "int8"),
    ("max", "int8", "int8"),
    ("min", "timestamp", "timestamp"),
    ("max", "timestamp", "timestamp"),
    ("min", "date", "date"),
    ("max", "date", "date"),
    ("min", "timestamptz", "timestamptz"),
    ("max", "timestamptz", "timestamptz"),
    ("min", "int2", "int2"),
    ("max", "int2", "int2"),
    ("min", "int4", "int4"),
    ("max", "int4", "int4"),
    ("min", "numeric", "numeric"),
    ("max", "numeric", "numeric"),
    ("min", "float8", "float8"),
    ("max", "float8", "float8"),
    ("min", "text", "text"),
    ("max", "text", "text"),
    ("min", "bpchar", "bpchar"),
    ("max", "bpchar", "bpchar"),
    ("min", "time", "time"),
    ("max", "time", "time"),
    ("min", "interval", "interval"),
    ("max", "interval", "interval"),
];

/// Only immutable timestamp-without-time-zone day transforms.
pub const DAY_FUNCTION_SIGNATURES: &[(&str, &[&str], &str)] = &[
    ("date_trunc", &["text", "timestamp"], "timestamp"),
    ("date", &["timestamp"], "date"),
];

/// Appended after the day functions to preserve every smaller catalog prefix.
pub const ADDITIONAL_AGGREGATE_SIGNATURES: &[(&str, &str, &str)] = &[("sum", "int4", "int8")];
/// Numeric ABS only: no float/integer overload is implicitly granted.
pub const ADDITIONAL_SCALAR_SIGNATURES: &[(&str, &[&str], &str)] = &[
    ("abs", &["numeric"], "numeric"),
    ("to_char", &["timestamp", "text"], "text"),
    ("extract", &["text", "date"], "numeric"),
    ("extract", &["text", "timestamp"], "numeric"),
    ("extract", &["text", "interval"], "numeric"),
    ("date_part", &["text", "timestamp"], "float8"),
    ("date_part", &["text", "interval"], "float8"),
    ("lower", &["text"], "text"),
    ("upper", &["text"], "text"),
    ("length", &["text"], "int4"),
    ("substring", &["text", "int4"], "text"),
    ("substring", &["text", "int4", "int4"], "text"),
    ("round", &["numeric", "int4"], "numeric"),
    ("round", &["float8"], "float8"),
    ("abs", &["int2"], "int2"),
    ("abs", &["int4"], "int4"),
    ("abs", &["int8"], "int8"),
    ("abs", &["float8"], "float8"),
];

/// Result inference is closed over the same signatures the live loader verifies.
/// It is not authority to execute a function missing from the supplied catalog.
pub(crate) fn builtin_function_result_type(function: &ResolvedFunction) -> Option<u32> {
    use postgres_types::Type;
    if function.schema != "pg_catalog" {
        return None;
    }
    let args = &function.argument_type_oids;
    if function.name == "count" {
        return ((function.aggregate_star && args.is_empty())
            || (!function.aggregate_star && args == &[Type::ANY.oid()]))
            .then_some(Type::INT8.oid());
    }
    if function.aggregate_star {
        return None;
    }
    if let [argument] = args.as_slice()
        && let Some(kind) = Type::from_oid(*argument)
        && let Some((_, _, result)) = UNARY_AGGREGATE_SIGNATURES
            .iter()
            .chain(ADDITIONAL_AGGREGATE_SIGNATURES)
            .find(|(name, input, _)| *name == function.name && *input == kind.name())
    {
        return Some(match *result {
            "numeric" => Type::NUMERIC.oid(),
            "int8" => Type::INT8.oid(),
            _ => *argument,
        });
    }
    DAY_FUNCTION_SIGNATURES
        .iter()
        .chain(ADDITIONAL_SCALAR_SIGNATURES)
        .find(|(name, inputs, _)| {
            *name == function.name
                && inputs.len() == args.len()
                && inputs
                    .iter()
                    .zip(args)
                    .all(|(name, oid)| Type::from_oid(*oid).is_some_and(|ty| ty.name() == *name))
        })
        .and_then(|(_, _, result)| {
            [
                Type::INT2,
                Type::INT4,
                Type::INT8,
                Type::NUMERIC,
                Type::FLOAT8,
                Type::TEXT,
                Type::DATE,
                Type::TIMESTAMP,
            ]
            .into_iter()
            .find(|ty| ty.name() == *result)
            .map(|ty| ty.oid())
        })
}

/// Type OID used for integer literals. Not a hard-coded PostgreSQL OID.
pub const INTEGER_LITERAL_TYPE: &str = "int4";

/// Type OID used for text literals.
pub const TEXT_LITERAL_TYPE: &str = "text";

/// Type OID used for boolean literals.
pub const BOOLEAN_LITERAL_TYPE: &str = "bool";

const PHYSICAL_SCHEMA: &str = "_oo";
const ABS_MAX_TABLES: u32 = 32;
const ABS_MAX_COLUMNS: u32 = 512;
/// Shared capacity for the complete builtin function catalog.
pub const FUNCTION_CATALOG_LIMIT: u32 = 2
    + UNARY_AGGREGATE_SIGNATURES.len() as u32
    + DAY_FUNCTION_SIGNATURES.len() as u32
    + ADDITIONAL_AGGREGATE_SIGNATURES.len() as u32
    + ADDITIONAL_SCALAR_SIGNATURES.len() as u32;
const ABS_MAX_FUNCTIONS: u32 = FUNCTION_CATALOG_LIMIT;
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
    /// Maximum function rows, in stable signature order. One loads only COUNT(*).
    /// Use [`FUNCTION_CATALOG_LIMIT`] for the complete supported function set.
    pub max_functions: u32,
    /// Maximum operator rows. Must cover the comparison and arithmetic sets.
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
    /// The role's publication fence is not open for the supplied Pack.
    PublicationBindingUnavailable,
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
            Self::PublicationBindingUnavailable => {
                formatter.write_str("publication binding is not available")
            }
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

#[derive(Clone)]
struct BoundInput {
    source: String,
    native_namespace: bool,
    role: String,
    config_digest: String,
    generation: i64,
    tables: Vec<TableBound>,
}

#[derive(Clone)]
struct TableBound {
    source: String,
    namespace: String,
    name: String,
    config_digest: String,
    generation: i64,
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
    load_policy_catalog_bound_source(client, identity, policies, limits, None, None, None).await
}

/// Authentication project stays unchanged; native source identity is independently verified by the native reviewed adapter.
pub async fn load_policy_catalog_for_source(
    client: &mut Client,
    identity: &PolicyIdentity,
    policies: &[CompiledPolicy],
    limits: &CatalogLimits,
    source: &str,
) -> Result<PolicyCatalog, CatalogError> {
    load_policy_catalog_bound_source(client, identity, policies, limits, Some(source), None, None)
        .await
}
/// Load one atomically verified native collection with independent per-table versions.
pub async fn load_policy_catalog_for_source_vector(
    client: &mut Client,
    identity: &PolicyIdentity,
    policies: &[CompiledPolicy],
    limits: &CatalogLimits,
    source: &str,
    vector: Option<&Value>,
    catalog: Option<&Value>,
) -> Result<PolicyCatalog, CatalogError> {
    load_policy_catalog_bound_source(
        client,
        identity,
        policies,
        limits,
        Some(source),
        vector,
        catalog,
    )
    .await
}
async fn load_policy_catalog_bound_source(
    client: &mut Client,
    identity: &PolicyIdentity,
    policies: &[CompiledPolicy],
    limits: &CatalogLimits,
    source: Option<&str>,
    vector: Option<&Value>,
    catalog: Option<&Value>,
) -> Result<PolicyCatalog, CatalogError> {
    let bound = bound_input(identity, policies, limits, source, vector, catalog)?;
    require_tls(client).await?;
    let tx = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .map_err(db_error)?;
    tx.batch_execute("SET LOCAL search_path TO pg_catalog; SET LOCAL statement_timeout = '30s'; SET LOCAL jit = off")
        .await.map_err(db_error)?;
    ensure_snapshot(&tx).await?;
    match load_in_snapshot(&tx, identity, &bound, limits, vector, catalog).await {
        Ok(catalog) => {
            ensure_snapshot(&tx).await?;
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

fn bound_input(
    identity: &PolicyIdentity,
    policies: &[CompiledPolicy],
    limits: &CatalogLimits,
    source: Option<&str>,
    vector: Option<&Value>,
    catalog: Option<&Value>,
) -> Result<BoundInput, CatalogError> {
    let library = catalog.is_some_and(|c| c["protocol"] == "oo.gateway.library-binding/v1");
    let mut bound = prepare_bound(identity, policies, limits, vector.is_some(), library)?;
    if let Some(source) = source {
        if !closed_identity(source) && !(vector.is_some() && source.is_empty()) {
            return Err(CatalogError::IdentityMismatch);
        }
        bound.source = source.to_owned();
        for table in &mut bound.tables {
            table.source = source.to_owned();
        }
        bound.native_namespace = true;
    }
    if let Some(vector) = vector {
        bind_vector_sources(
            &mut bound,
            vector,
            catalog.ok_or(CatalogError::IdentityMismatch)?,
        )?;
    }
    Ok(bound)
}

/// Recheck grants, mappings and publication fences in the SQL executor's snapshot.
/// The executor owns transaction settings and the remaining timeout budget.
pub(crate) async fn load_policy_catalog_in_transaction(
    tx: &Transaction<'_>,
    identity: &PolicyIdentity,
    policies: &[CompiledPolicy],
    limits: &CatalogLimits,
    source: Option<&str>,
    vector: Option<&Value>,
    catalog: Option<&Value>,
) -> Result<PolicyCatalog, CatalogError> {
    let bound = bound_input(identity, policies, limits, source, vector, catalog)?;
    load_in_snapshot(tx, identity, &bound, limits, vector, catalog).await
}

fn bind_vector_sources(
    bound: &mut BoundInput,
    vector: &Value,
    catalog: &Value,
) -> Result<(), CatalogError> {
    let entries = vector.as_array().ok_or(CatalogError::IdentityMismatch)?;
    if entries.len() != bound.tables.len()
        || catalog["entries"]
            .as_array()
            .is_none_or(|items| items.len() != entries.len())
    {
        return Err(CatalogError::IdentityMismatch);
    }
    let entries = index_vector(vector)?;
    for table in &mut bound.tables {
        let dataset = format!("{}.{}", table.namespace, table.name);
        let entry = entries
            .get(dataset.as_str())
            .ok_or(CatalogError::IdentityMismatch)?;
        let table_source = entry["source_id"]
            .as_str()
            .ok_or(CatalogError::IdentityMismatch)?;
        if !closed_identity(table_source) {
            return Err(CatalogError::IdentityMismatch);
        }
        table.source = table_source.to_owned();
    }
    Ok(())
}

/// Request-local lookup only; it holds no authority and never outlives the input.
pub(crate) fn index_vector(vector: &Value) -> Result<BTreeMap<&str, &Value>, CatalogError> {
    let mut by_dataset = BTreeMap::new();
    for entry in vector.as_array().ok_or(CatalogError::IdentityMismatch)? {
        let dataset = entry["dataset"]
            .as_str()
            .ok_or(CatalogError::IdentityMismatch)?;
        if by_dataset.insert(dataset, entry).is_some() {
            return Err(CatalogError::IdentityMismatch);
        }
    }
    Ok(by_dataset)
}

#[cfg(test)]
mod vector_index_tests {
    use super::*;

    #[test]
    fn request_local_vector_index_preserves_exact_identity_and_refuses_ambiguity() {
        let vector = serde_json::json!([
            {"dataset":"app.b","source_id":"source_b"},
            {"dataset":"app.a","source_id":"source_a"}
        ]);
        let indexed = index_vector(&vector).unwrap();
        assert_eq!(indexed["app.a"], &vector[1]);
        assert_eq!(indexed["app.b"], &vector[0]);
        assert!(!indexed.contains_key("APP.A"));
        assert!(
            index_vector(&serde_json::json!([{"dataset":"app.a"},{"dataset":"app.a"}])).is_err()
        );
        for invalid in [
            serde_json::json!(null),
            serde_json::json!([{}]),
            serde_json::json!([{"dataset":1}]),
        ] {
            assert!(index_vector(&invalid).is_err());
        }
    }
}

#[cfg(test)]
fn prepare(
    identity: &PolicyIdentity,
    policies: &[CompiledPolicy],
    limits: &CatalogLimits,
    native_vector: bool,
) -> Result<BoundInput, CatalogError> {
    prepare_bound(identity, policies, limits, native_vector, false)
}

fn prepare_bound(
    identity: &PolicyIdentity,
    policies: &[CompiledPolicy],
    limits: &CatalogLimits,
    native_vector: bool,
    library: bool,
) -> Result<BoundInput, CatalogError> {
    if policies.is_empty() {
        return Err(CatalogError::EmptyInput);
    }
    if library {
        check_library_limits(limits, policies.len())?;
    } else {
        check_limits(limits, policies.len())?;
    }
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
            || (!native_vector && policy.config_digest != first.config_digest)
            || (!native_vector && policy.generation != first.generation)
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
            source: first.project.clone(),
            namespace: policy.namespace.clone(),
            name: policy.table.clone(),
            config_digest: policy.config_digest.clone(),
            generation: policy.generation,
            columns: policy
                .columns
                .iter()
                .map(|column| column.name.clone())
                .collect(),
        });
    }
    Ok(BoundInput {
        source: first.project.clone(),
        native_namespace: false,
        role: first.role.clone(),
        config_digest: first.config_digest.clone(),
        generation: first.generation,
        tables,
    })
}

fn check_limits(limits: &CatalogLimits, tables: usize) -> Result<(), CatalogError> {
    check_capacity(limits, tables, ABS_MAX_TABLES, ABS_MAX_COLUMNS)
}

fn check_library_limits(limits: &CatalogLimits, tables: usize) -> Result<(), CatalogError> {
    check_capacity(limits, tables, 4096, 4096)
}

fn check_capacity(
    limits: &CatalogLimits,
    tables: usize,
    max_tables: u32,
    max_columns: u32,
) -> Result<(), CatalogError> {
    let Some(operators) = u32::try_from(required_operator_count()).ok() else {
        return Err(CatalogError::InvalidLimit);
    };
    if limits.max_tables == 0 || limits.max_tables > max_tables {
        return Err(CatalogError::InvalidLimit);
    }
    if tables > usize::try_from(limits.max_tables).unwrap_or(0) {
        return Err(CatalogError::LimitExceeded);
    }
    if limits.max_columns == 0 || limits.max_columns > max_columns {
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

/// Complete operator count; callers may not request a partial signature set.
pub fn required_operator_count() -> usize {
    COMPARISON_OPERATORS.len() * COMPARISON_SIGNATURES.len()
        + ARITHMETIC_OPERATORS.len() * ARITHMETIC_SIGNATURES.len()
        + PATTERN_OPERATORS.len()
}

fn whitelist_tokens_ok() -> bool {
    SUPPORTED_TYPES.iter().all(|name| type_name_ok(name))
        && COMPARISON_SIGNATURES
            .iter()
            .all(|(left, right)| type_name_ok(left) && type_name_ok(right))
        && COMPARISON_OPERATORS
            .iter()
            .all(|name| operator_name_ok(name))
        && ARITHMETIC_SIGNATURES.iter().all(|(left, right, result)| {
            [left, right, result]
                .iter()
                .all(|name| SUPPORTED_TYPES.contains(name))
        })
        && ARITHMETIC_OPERATORS
            .iter()
            .all(|name| operator_name_ok(name))
        && PATTERN_OPERATORS.iter().all(|name| operator_name_ok(name))
        && UNARY_AGGREGATE_SIGNATURES
            .iter()
            .all(|(name, argument, result)| {
                matches!(*name, "sum" | "min" | "max")
                    && SUPPORTED_TYPES.contains(argument)
                    && SUPPORTED_TYPES.contains(result)
            })
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
    COMPARISON_OPERATORS.contains(&value)
        || ARITHMETIC_OPERATORS.contains(&value)
        || PATTERN_OPERATORS.contains(&value)
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

fn table_scope(bound: &BoundInput, table: &TableBound) -> BoundInput {
    BoundInput {
        source: table.source.clone(),
        config_digest: table.config_digest.clone(),
        generation: table.generation,
        native_namespace: bound.native_namespace,
        role: bound.role.clone(),
        tables: Vec::new(),
    }
}

// One indexed read covers every mapping row the bound tables need. Per-table
// and per-column lookups then run in memory with the same absence, ambiguity,
// and lookup-equality rules as the row-by-row checks.
type MappingKey = (String, String, i64, String, String, String, String);
type ColumnMappings = BTreeMap<String, (String, bool)>;
type TableMappings = BTreeMap<String, ColumnMappings>;
type NamespaceMappings = BTreeMap<String, TableMappings>;
type LevelMappings = BTreeMap<String, NamespaceMappings>;
type GenerationMappings = BTreeMap<i64, LevelMappings>;
type DigestMappings = BTreeMap<String, GenerationMappings>;

struct MappingIndex {
    // Shared prefixes are stored once, and each lookup borrows its keys rather
    // than allocating seven strings for every raw/encoded alias combination.
    rows: BTreeMap<String, DigestMappings>,
}

impl MappingIndex {
    fn insert(&mut self, key: MappingKey, value: (String, bool)) -> Option<(String, bool)> {
        let (source, digest, generation, level, namespace, table, column) = key;
        self.rows
            .entry(source)
            .or_default()
            .entry(digest)
            .or_default()
            .entry(generation)
            .or_default()
            .entry(level)
            .or_default()
            .entry(namespace)
            .or_default()
            .entry(table)
            .or_default()
            .insert(column, value)
    }

    async fn load(tx: &Transaction<'_>, bound: &BoundInput) -> Result<Self, CatalogError> {
        let mut sources = Vec::new();
        let mut digests = Vec::new();
        let mut generations = Vec::new();
        let mut namespaces = Vec::new();
        let mut tables = Vec::new();
        let mut seen = BTreeSet::new();
        for table in &bound.tables {
            for namespace in mapping_keys(&table.namespace, bound.native_namespace) {
                for name in mapping_keys(&table.name, bound.native_namespace) {
                    if seen.insert((
                        table.source.clone(),
                        table.config_digest.clone(),
                        table.generation,
                        namespace.clone(),
                        name.clone(),
                    )) {
                        sources.push(table.source.clone());
                        digests.push(table.config_digest.clone());
                        generations.push(table.generation);
                        namespaces.push(namespace.clone());
                        tables.push(name);
                    }
                }
            }
        }
        let rows = tx.query(
            "SELECT source, config_digest, run_generation, level, logical_namespace, logical_table, logical_column, physical_name, \
                    physical_name = _oo.lookup_physical_mapping(source, config_digest, run_generation, \
                        CASE level WHEN 'n' THEN 'namespace' WHEN 't' THEN 'table' ELSE 'column' END, \
                        logical_namespace, logical_table, logical_column) \
             FROM _oo.physical_mappings \
             WHERE (source, config_digest, run_generation, logical_namespace, logical_table) IN \
                (SELECT source,digest,generation,namespace,table_name \
                 FROM unnest($1::text[],$2::text[],$3::int8[],$4::text[],$5::text[]) w(source,digest,generation,namespace,table_name) \
                 UNION SELECT source,digest,generation,namespace,'' \
                 FROM unnest($1::text[],$2::text[],$3::int8[],$4::text[]) w(source,digest,generation,namespace))",
            &[&sources, &digests, &generations, &namespaces, &tables],
        ).await.map_err(db_error)?;
        let mut index = Self {
            rows: BTreeMap::new(),
        };
        for row in &rows {
            let key = (
                row.get::<_, String>(0),
                row.get::<_, String>(1),
                row.get::<_, i64>(2),
                row.get::<_, String>(3),
                row.get::<_, String>(4),
                row.get::<_, String>(5),
                row.get::<_, String>(6),
            );
            if index
                .insert(key, (row.get::<_, String>(7), row.get::<_, bool>(8)))
                .is_some()
            {
                return Err(CatalogError::AmbiguousMapping);
            }
        }
        Ok(index)
    }

    fn resolve(
        &self,
        source: &str,
        digest: &str,
        generation: i64,
        level: MapLevel,
        native: bool,
        (namespace, table, column): (&str, &str, &str),
    ) -> Result<String, CatalogError> {
        let mut found = None;
        let mut count = 0;
        let namespaces = self
            .rows
            .get(source)
            .and_then(|m| m.get(digest))
            .and_then(|m| m.get(&generation))
            .and_then(|m| m.get(level.letter))
            .ok_or(CatalogError::MappingAbsent)?;
        let table_keys = mapping_keys(table, native);
        let column_keys = mapping_keys(column, native);
        for ns in mapping_keys(namespace, native) {
            let Some(tables) = namespaces.get(ns.as_str()) else {
                continue;
            };
            for tbl in &table_keys {
                let Some(columns) = tables.get(tbl.as_str()) else {
                    continue;
                };
                for col in &column_keys {
                    if let Some(value) = columns.get(col.as_str()) {
                        count += 1;
                        found = Some(value);
                    }
                }
            }
        }
        if count != 1 {
            return if count == 0 {
                Err(CatalogError::MappingAbsent)
            } else {
                Err(CatalogError::AmbiguousMapping)
            };
        }
        let (stored, looked_ok) = found.ok_or(CatalogError::MappingAbsent)?;
        if !*looked_ok || !physical_ok(stored, level.prefix) {
            return Err(CatalogError::AmbiguousMapping);
        }
        Ok(stored.clone())
    }
}

fn mapped_columns(
    index: &MappingIndex,
    bound: &BoundInput,
    table: &TableBound,
) -> Result<Vec<(String, String)>, CatalogError> {
    table
        .columns
        .iter()
        .map(|column| {
            index
                .resolve(
                    &bound.source,
                    &bound.config_digest,
                    bound.generation,
                    LEVEL_COLUMN,
                    bound.native_namespace,
                    (&table.namespace, &table.name, column),
                )
                .map(|physical| (column.clone(), physical))
        })
        .collect()
}

async fn load_in_snapshot(
    tx: &Transaction<'_>,
    identity: &PolicyIdentity,
    bound: &BoundInput,
    limits: &CatalogLimits,
    vector: Option<&Value>,
    catalog: Option<&Value>,
) -> Result<PolicyCatalog, CatalogError> {
    let retained_library =
        catalog.is_some_and(|c| c["protocol"] == "oo.gateway.library-binding/v1");
    let index = MappingIndex::load(tx, bound).await?;
    // A newer mapping is ingestion, not publication. LibraryProofs below
    // requires exact membership in the current ready immutable receipt instead.
    if !retained_library {
        snapshot::ensure_generations(tx, bound, vector.is_some()).await?;
    }
    ensure_role(tx, &bound.role).await?;
    let types = load_types(tx).await?;
    let operators = load_operators(tx, &types, limits).await?;
    let functions = load_functions(tx, &types, limits).await?;
    let physical = snapshot::Relations::load(tx, bound, &index, limits).await?;
    let entries = vector.map(index_vector).transpose()?;
    let library = if let (Some(entries), Some(catalog)) = (entries.as_ref(), catalog)
        && catalog["protocol"] == "oo.gateway.library-binding/v1"
    {
        Some(snapshot::LibraryProofs::load(tx, bound, entries, catalog, &index, &physical).await?)
    } else {
        None
    };
    let mut relations = Vec::with_capacity(bound.tables.len());
    for table in &bound.tables {
        let scoped = table_scope(bound, table);
        if let Some(entries) = &entries {
            verify_native_table_vector(
                tx,
                &scoped,
                table,
                entries,
                catalog.ok_or(CatalogError::IdentityMismatch)?,
                (&index, &physical),
                library.as_ref(),
            )
            .await?;
        }
        if let Some(relation) = load_relation(&scoped, table, &types, limits, &index, &physical)? {
            relations.push(relation);
        }
    }
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

async fn verify_native_table_vector(
    tx: &Transaction<'_>,
    bound: &BoundInput,
    table: &TableBound,
    entries: &BTreeMap<&str, &Value>,
    catalog: &Value,
    (index, physical_index): (&MappingIndex, &snapshot::Relations),
    library: Option<&snapshot::LibraryProofs>,
) -> Result<(), CatalogError> {
    if catalog["protocol"] == "oo.gateway.library-binding/v1" {
        return verify_library_table_vector(
            bound,
            table,
            entries,
            index,
            physical_index,
            library.ok_or(CatalogError::IdentityMismatch)?,
        );
    }
    let dataset = format!("{}.{}", table.namespace, table.name);
    let entry = entries
        .get(dataset.as_str())
        .ok_or(CatalogError::IdentityMismatch)?;
    let text = |key: &str| entry[key].as_str().ok_or(CatalogError::IdentityMismatch);
    if text("source_id")? != bound.source
        || text("config_digest")? != table.config_digest
        || entry["source_generation"].as_i64() != Some(table.generation)
    {
        return Err(CatalogError::IdentityMismatch);
    }
    let namespace = index.resolve(
        &bound.source,
        &bound.config_digest,
        bound.generation,
        LEVEL_NAMESPACE,
        bound.native_namespace,
        (&table.namespace, "", ""),
    )?;
    let physical = index.resolve(
        &bound.source,
        &bound.config_digest,
        bound.generation,
        LEVEL_TABLE,
        bound.native_namespace,
        (&table.namespace, &table.name, ""),
    )?;
    if namespace != text("stored_namespace")? || physical != text("stored_physical")? {
        return Err(CatalogError::IdentityMismatch);
    }
    let target_generation = entry["target_generation"]
        .as_i64()
        .ok_or(CatalogError::IdentityMismatch)?;
    let row = tx.query_one(
        "SELECT count(*)::int8 FROM _oo.full_import_run r JOIN _oo.full_import_table t USING(run_id) \
         WHERE r.run_id::text=$1 AND r.source_id=$2 AND r.source_generation=$3 \
           AND r.target_generation=$4 AND r.config_digest=$5 AND r.type_digest=$6 \
           AND r.input_digest=$7 AND r.sealed_at IS NOT NULL \
           AND r.state IN ('awaiting_publish','published') \
           AND t.schema_ident=$8 AND t.table_ident=$9 AND t.physical_name=$10",
        &[&text("run_id")?, &bound.source, &table.generation, &target_generation,
          &table.config_digest, &text("type_digest")?, &text("input_digest")?,
          &table.namespace, &table.name, &physical],
    ).await.map_err(db_error)?;
    let count: i64 = row.get(0);
    if count != 1 {
        return Err(CatalogError::IdentityMismatch);
    }
    let catalog_entries = catalog["entries"]
        .as_array()
        .ok_or(CatalogError::IdentityMismatch)?;
    let matching = catalog_entries
        .iter()
        .filter(|candidate| {
            candidate["run_id"] == entry["run_id"]
                && candidate["tables"][0]["logical_schema"] == table.namespace
                && candidate["tables"][0]["logical_table"] == table.name
        })
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(CatalogError::IdentityMismatch);
    }
    let catalog_entry = matching[0];
    if catalog_entry["schema_digest"] != entry["schema_digest"]
        || catalog_entry["source_id"] != entry["source_id"]
        || catalog_entry["type_digest"] != entry["type_digest"]
        || catalog_entry["input_digest"] != entry["input_digest"]
        || catalog_entry["source_generation"] != entry["source_generation"]
        || catalog_entry["target_generation"] != entry["target_generation"]
        || catalog_entry["config_digest"] != entry["config_digest"]
        || catalog_entry["tables"][0]["stored_physical"] != physical
        || catalog_entry["tables"][0]["stored_namespace"] != namespace
    {
        return Err(CatalogError::IdentityMismatch);
    }
    let columns = catalog_entry["tables"][0]["columns"]
        .as_array()
        .ok_or(CatalogError::IdentityMismatch)?;
    let relation = &physical_index.get(&namespace, &physical)?.relation;
    let rows = tx
        .query(
            "SELECT a.attname::text,a.atttypid::int8,a.atttypmod::int4 \
         FROM pg_catalog.pg_attribute a \
         WHERE a.attrelid=($1::int4)::oid \
           AND a.attnum>0 AND NOT a.attisdropped AND a.attname <> '_oo_pos' \
         ORDER BY a.attnum",
            &[&relation.oid],
        )
        .await
        .map_err(db_error)?;
    if rows.len() != columns.len() {
        return Err(CatalogError::IdentityMismatch);
    }
    let mut preimage = String::new();
    oo_core::sealed_import::push_schema_table(&mut preimage, &table.namespace, &table.name);
    for (row, column) in rows.iter().zip(columns) {
        let physical_name: String = row.get(0);
        let oid: i64 = row.get(1);
        let typmod: i32 = row.get(2);
        let logical_name = column["logical_name"]
            .as_str()
            .ok_or(CatalogError::IdentityMismatch)?;
        if column["stored_physical"] != physical_name
            || column["oid"].as_i64() != Some(oid)
            || column["typmod"].as_i64() != Some(i64::from(typmod))
        {
            return Err(CatalogError::IdentityMismatch);
        }
        let caveat = if column["caveat"].is_null() {
            None
        } else {
            Some(
                column["caveat"]
                    .as_str()
                    .ok_or(CatalogError::IdentityMismatch)?,
            )
        };
        oo_core::sealed_import::push_schema_column(
            &mut preimage,
            logical_name,
            oid,
            typmod,
            caveat,
        );
    }
    let calculated = format!("{:x}", Sha256::digest(preimage.as_bytes()));
    if catalog_entry["schema_digest"] != calculated {
        return Err(CatalogError::IdentityMismatch);
    }
    Ok(())
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
    let (names, lefts, rights, results) = wanted_operators();
    let expected = i64::try_from(names.len()).map_err(|_| CatalogError::LimitExceeded)?;
    if expected > i64::from(limits.max_operators) {
        return Err(CatalogError::LimitExceeded);
    }
    let params: [&(dyn ToSql + Sync); 4] = [&names, &lefts, &rights, &results];
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
                        left_type.typname, right_type.typname, result_type.typname, \
                        operator.oprresult::int4 {OPERATOR_FROM} \
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
            results[index].clone(),
        ));
    }
    let mut operators = Vec::with_capacity(rows.len());
    for row in rows {
        let name: String = row.get(2);
        let left_name: String = row.get(3);
        let right_name: String = row.get(4);
        let result_name: String = row.get(5);
        if !wanted.remove(&(
            name.clone(),
            left_name.clone(),
            right_name.clone(),
            result_name.clone(),
        )) {
            return Err(CatalogError::AmbiguousObject);
        }
        let left = *types.get(&left_name).ok_or(CatalogError::MissingObject)?;
        let right = *types.get(&right_name).ok_or(CatalogError::MissingObject)?;
        let result = oid_from_i32(row.get(6))?;
        if types.get(&result_name) != Some(&result) {
            return Err(CatalogError::MissingObject);
        }
        operators.push(ResolvedOperator {
            oid: oid_from_i32(row.get(0))?,
            procedure_oid: oid_from_i32(row.get(1))?,
            schema: "pg_catalog".to_string(),
            name,
            left_type_oid: left,
            right_type_oid: right,
            result_type_oid: result,
        });
    }
    if !wanted.is_empty() {
        return Err(CatalogError::MissingObject);
    }
    Ok(operators)
}

fn wanted_operators() -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    let mut names = Vec::new();
    let mut lefts = Vec::new();
    let mut rights = Vec::new();
    let mut results = Vec::new();
    for (left, right) in COMPARISON_SIGNATURES {
        for operator in COMPARISON_OPERATORS {
            names.push((*operator).to_string());
            lefts.push((*left).to_string());
            rights.push((*right).to_string());
            results.push(BOOLEAN_LITERAL_TYPE.to_string());
        }
    }
    for (left, right, result) in ARITHMETIC_SIGNATURES {
        for operator in ARITHMETIC_OPERATORS {
            names.push((*operator).to_string());
            lefts.push((*left).to_string());
            rights.push((*right).to_string());
            results.push((*result).to_string());
        }
    }
    for operator in PATTERN_OPERATORS {
        names.push((*operator).to_string());
        lefts.push("text".into());
        rights.push("text".into());
        results.push("bool".into());
    }
    (names, lefts, rights, results)
}

const OPERATOR_FROM: &str = "\
FROM unnest($1::text[], $2::text[], $3::text[], $4::text[]) \
 AS wanted(oprname, left_name, right_name, result_name) \
JOIN pg_catalog.pg_operator AS operator \
  ON operator.oprname = wanted.oprname \
 AND operator.oprkind = 'b' \
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
JOIN pg_catalog.pg_type AS result_type \
  ON result_type.oid = operator.oprresult \
 AND result_type.typname = wanted.result_name \
 AND result_type.typtype = 'b' \
JOIN pg_catalog.pg_namespace AS result_namespace \
  ON result_namespace.oid = result_type.typnamespace \
 AND result_namespace.nspname = 'pg_catalog' \
JOIN pg_catalog.pg_proc AS procedure \
  ON procedure.oid = operator.oprcode \
 AND procedure.prokind = 'f' \
 AND procedure.provolatile = 'i' \
 AND procedure.pronargs = 2 \
 AND NOT procedure.proretset \
 AND NOT procedure.prosecdef \
 AND procedure.provariadic = 0 \
 AND procedure.proconfig IS NULL \
 AND procedure.prorettype = operator.oprresult \
 AND procedure.proargtypes[0] = operator.oprleft \
 AND procedure.proargtypes[1] = operator.oprright \
JOIN pg_catalog.pg_namespace AS procedure_namespace \
  ON procedure_namespace.oid = procedure.pronamespace \
 AND procedure_namespace.nspname = 'pg_catalog' \
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

async fn load_functions(
    tx: &Transaction<'_>,
    types: &BTreeMap<String, u32>,
    limits: &CatalogLimits,
) -> Result<Vec<ResolvedFunction>, CatalogError> {
    let mut functions = load_count(tx, types, limits).await?;
    if limits.max_functions > 1 {
        functions.push(load_count_any(tx, types).await?);
    }
    let unary_count = limits.max_functions.saturating_sub(2) as usize;
    let signatures =
        &UNARY_AGGREGATE_SIGNATURES[..unary_count.min(UNARY_AGGREGATE_SIGNATURES.len())];
    functions.extend(load_unary_aggregates(tx, types, signatures).await?);
    let day_count = limits.max_functions.saturating_sub(functions.len() as u32) as usize;
    for &(name, arguments, result) in DAY_FUNCTION_SIGNATURES.iter().take(day_count) {
        functions.push(load_scalar_function(tx, types, name, arguments, result).await?);
    }
    let remaining = limits.max_functions.saturating_sub(functions.len() as u32) as usize;
    functions.extend(
        load_unary_aggregates(
            tx,
            types,
            &ADDITIONAL_AGGREGATE_SIGNATURES
                [..remaining.min(ADDITIONAL_AGGREGATE_SIGNATURES.len())],
        )
        .await?,
    );
    let remaining = limits.max_functions.saturating_sub(functions.len() as u32) as usize;
    for &(name, arguments, result) in ADDITIONAL_SCALAR_SIGNATURES.iter().take(remaining) {
        functions.push(load_scalar_function(tx, types, name, arguments, result).await?);
    }
    Ok(functions)
}

async fn load_scalar_function(
    tx: &Transaction<'_>,
    types: &BTreeMap<String, u32>,
    name: &str,
    arguments: &[&str],
    result: &str,
) -> Result<ResolvedFunction, CatalogError> {
    if !DAY_FUNCTION_SIGNATURES.contains(&(name, arguments, result))
        && !ADDITIONAL_SCALAR_SIGNATURES.contains(&(name, arguments, result))
    {
        return Err(CatalogError::MissingObject);
    }
    let oid = |kind| {
        types
            .get(kind)
            .and_then(|oid| i32::try_from(*oid).ok())
            .ok_or(CatalogError::MissingObject)
    };
    let argument_oids = arguments
        .iter()
        .map(|kind| oid(*kind))
        .collect::<Result<Vec<_>, _>>()?;
    let result_oid = oid(result)?;
    let rows = tx
        .query(
            "SELECT p.oid::int4 FROM pg_catalog.pg_proc p \
         JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace \
         JOIN pg_catalog.pg_language l ON l.oid=p.prolang \
         WHERE n.nspname='pg_catalog' AND p.proname=$1 AND p.prokind='f' \
           AND ((p.proname='to_char' AND p.provolatile='s' AND p.prosrc='timestamp_to_char') \
             OR (p.proname<>'to_char' AND p.provolatile='i')) \
           AND NOT p.proretset AND NOT p.prosecdef \
           AND p.provariadic=0 AND p.proconfig IS NULL \
           AND p.pronargs=cardinality($2::int4[]) \
           AND ARRAY(SELECT x::int4 FROM unnest(p.proargtypes) x)=$2::int4[] \
           AND p.prorettype=($3::int4)::oid AND l.lanname='internal' \
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.objid=p.oid \
               AND d.classid='pg_catalog.pg_proc'::regclass AND d.deptype='e') \
           AND ($1<>'date' OR EXISTS (SELECT 1 FROM pg_catalog.pg_cast c \
               WHERE c.castsource=($2::int4[])[1]::oid AND c.casttarget=p.prorettype \
                 AND c.castmethod='f' AND c.castcontext='a' AND c.castfunc=p.oid \
                 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.objid=c.oid \
                     AND d.classid='pg_catalog.pg_cast'::regclass AND d.deptype='e')))",
            &[&name, &argument_oids, &result_oid],
        )
        .await
        .map_err(db_error)?;
    let row = match rows.as_slice() {
        [row] => row,
        [] => return Err(CatalogError::MissingObject),
        _ => return Err(CatalogError::AmbiguousObject),
    };
    Ok(ResolvedFunction {
        oid: oid_from_i32(row.get(0))?,
        schema: "pg_catalog".into(),
        name: name.into(),
        argument_type_oids: argument_oids
            .into_iter()
            .map(oid_from_i32)
            .collect::<Result<_, _>>()?,
        aggregate_star: false,
    })
}

async fn load_unary_aggregates(
    tx: &Transaction<'_>,
    types: &BTreeMap<String, u32>,
    signatures: &[(&str, &str, &str)],
) -> Result<Vec<ResolvedFunction>, CatalogError> {
    if signatures.is_empty() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    let mut arguments = Vec::new();
    let mut results = Vec::new();
    for &(name, argument, result) in signatures {
        if !UNARY_AGGREGATE_SIGNATURES.contains(&(name, argument, result))
            && !ADDITIONAL_AGGREGATE_SIGNATURES.contains(&(name, argument, result))
        {
            return Err(CatalogError::MissingObject);
        }
        let oid = |kind| {
            types
                .get(kind)
                .and_then(|oid| i32::try_from(*oid).ok())
                .ok_or(CatalogError::MissingObject)
        };
        names.push(name);
        arguments.push(oid(argument)?);
        results.push(oid(result)?);
    }
    // A larger builtin set still costs two bounded catalog queries, not one
    // round trip per signature on every admission/recheck.
    let params: [&(dyn ToSql + Sync); 3] = [&names, &arguments, &results];
    let expected = signatures.len() as i64;
    let count_row = tx
        .query_one(
            &format!(
                "SELECT count(*)::int8, count(DISTINCT procedure.oid)::int8 {UNARY_AGGREGATE_FROM}"
            ),
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
                "SELECT procedure.oid::int4, procedure.proname, wanted.ordinal \
                      {UNARY_AGGREGATE_FROM} ORDER BY wanted.ordinal"
            ),
            &params,
        )
        .await
        .map_err(db_error)?;
    if rows.len() != signatures.len() {
        return Err(CatalogError::Snapshot);
    }
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let name: String = row.get(1);
            if name != names[index] || row.get::<_, i64>(2) != index as i64 + 1 {
                return Err(CatalogError::AmbiguousObject);
            }
            Ok(ResolvedFunction {
                oid: oid_from_i32(row.get(0))?,
                schema: "pg_catalog".to_string(),
                name,
                argument_type_oids: vec![oid_from_i32(arguments[index])?],
                aggregate_star: false,
            })
        })
        .collect()
}

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

async fn load_count_any(
    tx: &Transaction<'_>,
    types: &BTreeMap<String, u32>,
) -> Result<ResolvedFunction, CatalogError> {
    let int8_oid = types.get("int8").ok_or(CatalogError::MissingObject)?;
    let int8_i = i32::try_from(*int8_oid).map_err(|_| CatalogError::MissingObject)?;
    let name = AGGREGATE_STAR_NAME.to_string();
    let params: [&(dyn ToSql + Sync); 2] = [&name, &int8_i];
    let rows = tx
        .query(
            &format!("SELECT procedure.oid::int4, procedure.proargtypes[0]::int4 {COUNT_ANY_FROM}"),
            &params,
        )
        .await
        .map_err(db_error)?;
    match rows.as_slice() {
        [row] => Ok(ResolvedFunction {
            oid: oid_from_i32(row.get(0))?,
            schema: "pg_catalog".to_string(),
            name,
            argument_type_oids: vec![oid_from_i32(row.get(1))?],
            aggregate_star: false,
        }),
        [] => Err(CatalogError::MissingObject),
        _ => Err(CatalogError::AmbiguousObject),
    }
}

const COUNT_ANY_FROM: &str = "\
FROM pg_catalog.pg_proc AS procedure \
JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = procedure.pronamespace \
JOIN pg_catalog.pg_aggregate AS aggregate ON aggregate.aggfnoid = procedure.oid \
JOIN pg_catalog.pg_language AS language ON language.oid = procedure.prolang \
JOIN pg_catalog.pg_type AS argument ON argument.oid = procedure.proargtypes[0] \
WHERE namespace.nspname = 'pg_catalog' \
  AND procedure.proname = $1 \
  AND procedure.prokind = 'a' \
  AND procedure.pronargs = 1 \
  AND argument.typname = 'any' \
  AND argument.typtype = 'p' \
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

const UNARY_AGGREGATE_FROM: &str = "\
FROM unnest($1::text[], $2::int4[], $3::int4[]) WITH ORDINALITY \
  AS wanted(name, argument_oid, result_oid, ordinal) \
JOIN pg_catalog.pg_proc AS procedure ON procedure.proname = wanted.name \
JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = procedure.pronamespace \
JOIN pg_catalog.pg_aggregate AS aggregate ON aggregate.aggfnoid = procedure.oid \
JOIN pg_catalog.pg_language AS language ON language.oid = procedure.prolang \
JOIN pg_catalog.pg_type AS argument ON argument.oid = procedure.proargtypes[0] \
WHERE namespace.nspname = 'pg_catalog' \
  AND procedure.prokind = 'a' \
  AND procedure.pronargs = 1 \
  AND procedure.provolatile = 'i' \
  AND NOT procedure.proretset \
  AND NOT procedure.prosecdef \
  AND procedure.provariadic = 0 \
  AND procedure.proconfig IS NULL \
  AND procedure.prorettype = wanted.result_oid::oid \
  AND argument.oid = wanted.argument_oid::oid \
  AND aggregate.aggkind = 'n' \
  AND aggregate.aggnumdirectargs = 0 \
  AND language.lanname = 'internal' \
  AND NOT EXISTS ( \
      SELECT 1 FROM pg_catalog.pg_depend AS depend \
      WHERE depend.objid = procedure.oid \
        AND depend.classid = 'pg_catalog.pg_proc'::regclass \
        AND depend.deptype = 'e' \
    )";

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

fn load_relation(
    bound: &BoundInput,
    table: &TableBound,
    types: &BTreeMap<String, u32>,
    limits: &CatalogLimits,
    index: &MappingIndex,
    physical_index: &snapshot::Relations,
) -> Result<Option<VisibleRelation>, CatalogError> {
    let physical_schema = if bound.native_namespace {
        index.resolve(
            &bound.source,
            &bound.config_digest,
            bound.generation,
            LEVEL_NAMESPACE,
            bound.native_namespace,
            (&table.namespace, "", ""),
        )?
    } else {
        PHYSICAL_SCHEMA.to_owned()
    };
    let physical_table = index.resolve(
        &bound.source,
        &bound.config_digest,
        bound.generation,
        LEVEL_TABLE,
        bound.native_namespace,
        (&table.namespace, &table.name, ""),
    )?;
    let published = mapped_columns(index, bound, table)?;
    let physical = physical_index.get(&physical_schema, &physical_table)?;
    let relation = &physical.relation;
    let attributes = &physical.attributes;
    if attributes.len() > limits.max_columns as usize {
        return Err(CatalogError::LimitExceeded);
    }
    let mut by_name = BTreeMap::new();
    for item in attributes {
        if by_name.insert(item.attribute.name.clone(), item).is_some() {
            return Err(CatalogError::AmbiguousMapping);
        }
    }
    let mut visible = Vec::new();
    for (logical, physical) in &published {
        let item = by_name.get(physical).ok_or(CatalogError::MappingAbsent)?;
        let attribute = &item.attribute;
        if !supported_base(attribute, types) {
            return Err(CatalogError::UnsupportedType);
        }
        if item.allowed {
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
        physical_schema: relation.schema.clone(),
        physical_name: physical_table,
        columns,
        pending_unclassified: pending,
        estimated_rows: estimate_rows(relation.reltuples, limits.unknown_rows)?,
    }))
}

struct RelationRow {
    schema: String,
    oid: i32,
    reltuples: f32,
}

fn admitted_native_relation(kind: &str, native: bool, registered_overlay: bool) -> bool {
    kind == "r" || (kind == "v" && native && registered_overlay)
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

#[derive(Clone, Copy)]
struct MapLevel {
    letter: &'static str,
    prefix: &'static str,
}

const LEVEL_NAMESPACE: MapLevel = MapLevel {
    letter: "n",
    prefix: "pn_",
};
const LEVEL_TABLE: MapLevel = MapLevel {
    letter: "t",
    prefix: "pt_",
};
const LEVEL_COLUMN: MapLevel = MapLevel {
    letter: "c",
    prefix: "pc_",
};

// pgident/v1 may register a quoted original under q_<lowercase UTF-8 hex>.
// The trusted original still names the public catalog; a second stored key
// is accepted only when it is exactly reversible and the lookup is unique.
fn mapping_keys(original: &str, native: bool) -> Vec<String> {
    let mut keys = vec![original.to_owned()];
    if native && !original.is_empty() {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(2 + original.len() * 2);
        encoded.push_str("q_");
        for byte in original.bytes() {
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 15) as usize] as char);
        }
        if encoded != original {
            keys.push(encoded);
        }
    }
    keys
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

    #[test]
    fn only_receipted_native_overlays_are_admitted_as_views() {
        assert!(admitted_native_relation("r", false, false));
        assert!(admitted_native_relation("v", true, true));
        assert!(!admitted_native_relation("v", true, false));
        assert!(!admitted_native_relation("v", false, true));
        assert!(!admitted_native_relation("m", true, true));
        assert!(!admitted_native_relation("f", true, true));
    }

    #[test]
    fn native_mapping_key_preserves_original_and_reversible_quoted_name() {
        assert_eq!(mapping_keys("whCode", true), ["whCode", "q_7768436f6465"]);
        assert_eq!(mapping_keys("whCode", false), ["whCode"]);
        assert_eq!(mapping_keys("", true), [""]);
    }

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

    #[test]
    fn native_vector_scopes_each_table_to_its_real_source() {
        let make_table = |name: &str| TableBound {
            source: "aggregate".into(),
            namespace: "sales".into(),
            name: name.into(),
            config_digest: "a".repeat(64),
            generation: 3,
            columns: vec!["id".into()],
        };
        let mut bound = BoundInput {
            source: "aggregate".into(),
            native_namespace: true,
            role: "oo_r_sales".into(),
            config_digest: "a".repeat(64),
            generation: 3,
            tables: vec![make_table("east"), make_table("west")],
        };
        let vector = serde_json::json!([
            {"dataset":"sales.east","source_id":"east_source"},
            {"dataset":"sales.west","source_id":"west_source"}
        ]);
        let catalog = serde_json::json!({"entries":[{},{}]});
        bind_vector_sources(&mut bound, &vector, &catalog).unwrap();
        assert_eq!(
            bound
                .tables
                .iter()
                .map(|table| table.source.as_str())
                .collect::<Vec<_>>(),
            vec!["east_source", "west_source"]
        );
        let mut duplicate = vector.clone();
        duplicate[1]["dataset"] = serde_json::json!("sales.east");
        assert!(bind_vector_sources(&mut bound, &duplicate, &catalog).is_err());
        let mut missing_source = vector.clone();
        missing_source[1]["source_id"] = serde_json::Value::Null;
        assert!(bind_vector_sources(&mut bound, &missing_source, &catalog).is_err());
    }

    fn identity() -> PolicyIdentity {
        PolicyIdentity {
            project: "ledger".to_string(),
            role: "oo_r_clerk".to_string(),
            policy_id: "ab".repeat(32),
        }
    }

    #[test]
    fn bounded_numeric_admits_precision_1_through_76_and_the_exact_extra_bases() {
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
        assert!(grantable_base_type("numeric", typmod(39, 4)));
        assert!(grantable_base_type("numeric", typmod(76, 10)));
        assert_eq!(typmod(76, 10), 4_980_750);
        assert!(grantable_base_type("int8", -1));
        assert!(grantable_base_type("varchar", -1));
        assert!(grantable_base_type("float8", -1));
        assert!(grantable_base_type("timestamptz", -1));
        assert!(grantable_base_type("json", -1));
        assert!(!grantable_base_type("numeric", -1));
        assert!(!grantable_base_type("numeric", typmod(0, 0)));
        assert!(!grantable_base_type("numeric", typmod(77, 10)));
        assert!(!grantable_base_type("numeric", typmod(1000, 0)));
        assert!(grantable_base_type("bytea", -1));
        assert!(grantable_base_type("uuid", -1));
        assert!(!grantable_base_type("uuid", 0));
        assert!(!grantable_base_type("uuid", 4));
        assert!(grantable_base_type("bpchar", -1));
        assert!(grantable_base_type("bpchar", 10));
        assert!(!grantable_base_type("bpchar", 0));
        assert!(!grantable_base_type("bpchar", 1));
        assert!(!grantable_base_type("bpchar", 2));
        assert!(!grantable_base_type("bpchar", 3));
        assert!(!grantable_base_type("bpchar", 4));
        assert!(grantable_base_type("date", -1));
        assert!(!grantable_base_type("date", 0));
        assert!(!grantable_base_type("date", 4));
        assert!(grantable_base_type("timestamp", -1));
        assert!(!grantable_base_type("timestamp", 0));
        assert!(!grantable_base_type("timestamp", 6));
        assert!(grantable_base_type("jsonb", -1));
        assert!(!grantable_base_type("jsonb", 0));
        assert!(!grantable_base_type("jsonb", 4));
        assert!(!grantable_base_type("float4", -1));
        assert!(grantable_base_type("interval", -1));
        assert!(!grantable_base_type("interval", 0));
        assert!(grantable_base_type("time", -1));
        assert!(!grantable_base_type("time", 0));
        assert!(!grantable_base_type("xml", -1));
        assert_eq!(required_operator_count(), 124);
    }

    #[test]
    fn whitelist_covers_comparisons_and_exact_numeric_arithmetic() {
        assert_eq!(required_operator_count(), 124);
        assert!(COMPARISON_SIGNATURES.contains(&("date", "date")));
        assert!(COMPARISON_SIGNATURES.contains(&("interval", "interval")));
        assert!(COMPARISON_SIGNATURES.contains(&("timestamp", "timestamp")));
        assert!(COMPARISON_SIGNATURES.contains(&("timestamptz", "timestamptz")));
        assert!(!COMPARISON_SIGNATURES.iter().any(|(left, right)| {
            (["date", "timestamp", "timestamptz"].contains(left)
                || ["date", "timestamp", "timestamptz"].contains(right))
                && left != right
        }));
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
    fn operator_requests_bind_complete_unique_result_signatures() {
        let (names, lefts, rights, results) = wanted_operators();
        assert_eq!(names.len(), required_operator_count());
        assert_eq!(lefts.len(), names.len());
        assert_eq!(rights.len(), names.len());
        assert_eq!(results.len(), names.len());
        let mut seen = BTreeSet::new();
        for index in 0..names.len() {
            let name = names[index].as_str();
            let signature = (
                lefts[index].as_str(),
                rights[index].as_str(),
                results[index].as_str(),
            );
            assert!(seen.insert((name, signature)));
            if COMPARISON_OPERATORS.contains(&name) {
                assert_eq!(signature.2, BOOLEAN_LITERAL_TYPE);
                assert!(COMPARISON_SIGNATURES.contains(&(signature.0, signature.1)));
            } else if PATTERN_OPERATORS.contains(&name) {
                assert_eq!(signature, ("text", "text", "bool"));
            } else {
                assert!(ARITHMETIC_OPERATORS.contains(&name));
                assert!(ARITHMETIC_SIGNATURES.contains(&signature));
                for kind in [signature.0, signature.1, signature.2] {
                    assert!(["int2", "int4", "int8", "numeric", "float8"].contains(&kind));
                }
            }
        }
        let mut partial = limits();
        partial.max_operators = 123;
        assert_eq!(check_limits(&partial, 1), Err(CatalogError::InvalidLimit));
        partial.max_operators = 124;
        assert_eq!(check_limits(&partial, 1), Ok(()));
    }

    #[test]
    fn operator_lookup_checks_implementation_and_return_type() {
        // Protect both count and row lookup: they share this exact SQL predicate.
        for predicate in [
            "operator.oprkind = 'b'",
            "result_type.oid = operator.oprresult",
            "result_type.typname = wanted.result_name",
            "result_namespace.nspname = 'pg_catalog'",
            "procedure.oid = operator.oprcode",
            "procedure_namespace.nspname = 'pg_catalog'",
            "procedure.prokind = 'f'",
            "procedure.provolatile = 'i'",
            "procedure.pronargs = 2",
            "NOT procedure.proretset",
            "NOT procedure.prosecdef",
            "procedure.provariadic = 0",
            "procedure.proconfig IS NULL",
            "procedure.prorettype = operator.oprresult",
            "procedure.proargtypes[0] = operator.oprleft",
            "procedure.proargtypes[1] = operator.oprright",
            "language.lanname = 'internal'",
            "depend.classid = 'pg_catalog.pg_operator'::regclass",
            "depend.classid = 'pg_catalog.pg_proc'::regclass",
        ] {
            assert!(OPERATOR_FROM.contains(predicate), "{predicate}");
        }
        assert_eq!(OPERATOR_FROM.matches("depend.deptype = 'e'").count(), 2);
    }

    #[test]
    fn aggregate_catalog_has_exact_bounded_signatures() {
        assert_eq!(FUNCTION_CATALOG_LIMIT, 49);
        assert_eq!(UNARY_AGGREGATE_SIGNATURES.len(), 26);
        let unique = UNARY_AGGREGATE_SIGNATURES.iter().collect::<BTreeSet<_>>();
        assert_eq!(unique.len(), UNARY_AGGREGATE_SIGNATURES.len());
        for name in ["min", "max"] {
            for kind in [
                "int2",
                "int4",
                "int8",
                "numeric",
                "float8",
                "text",
                "bpchar",
                "date",
                "time",
                "timestamp",
                "timestamptz",
                "interval",
            ] {
                assert!(UNARY_AGGREGATE_SIGNATURES.contains(&(name, kind, kind)));
            }
        }
        for predicate in [
            "namespace.nspname = 'pg_catalog'",
            "procedure.proname = wanted.name",
            "procedure.prokind = 'a'",
            "procedure.pronargs = 1",
            "procedure.provolatile = 'i'",
            "NOT procedure.proretset",
            "NOT procedure.prosecdef",
            "procedure.provariadic = 0",
            "procedure.proconfig IS NULL",
            "procedure.prorettype = wanted.result_oid::oid",
            "argument.oid = wanted.argument_oid::oid",
            "aggregate.aggkind = 'n'",
            "aggregate.aggnumdirectargs = 0",
            "language.lanname = 'internal'",
            "depend.classid = 'pg_catalog.pg_proc'::regclass",
            "depend.deptype = 'e'",
        ] {
            assert!(UNARY_AGGREGATE_FROM.contains(predicate), "{predicate}");
        }
        let mut limits = limits();
        for budget in 1..=FUNCTION_CATALOG_LIMIT {
            limits.max_functions = budget;
            assert_eq!(check_limits(&limits, 1), Ok(()));
            assert_eq!(check_library_limits(&limits, 1), Ok(()));
        }
        for budget in [0, FUNCTION_CATALOG_LIMIT + 1] {
            limits.max_functions = budget;
            assert_eq!(check_limits(&limits, 1), Err(CatalogError::InvalidLimit));
            assert_eq!(
                check_library_limits(&limits, 1),
                Err(CatalogError::InvalidLimit)
            );
        }
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
            refused(prepare(&identity(), &[], &limits(), false)),
            CatalogError::EmptyInput
        );
        let mut wrong = identity();
        wrong.role = "oo_r_auditor".to_string();
        assert_eq!(
            refused(prepare(
                &wrong,
                std::slice::from_ref(&clerk),
                &limits(),
                false
            )),
            CatalogError::IdentityMismatch
        );
        let auditor = policy("auditor", vec!["amount"]);
        assert_eq!(
            refused(prepare(&identity(), &[clerk, auditor], &limits(), false)),
            CatalogError::MixedIdentity
        );
        let mut narrow = limits();
        narrow.max_operators = 1;
        assert_eq!(
            refused(prepare(
                &identity(),
                &[policy("clerk", vec!["amount"])],
                &narrow,
                false
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

#[cfg(test)]
mod library_capacity_tests {
    use super::*;
    #[test]
    fn library_catalog_ceiling_is_explicit_and_does_not_change_legacy_limits() {
        let mut limits = CatalogLimits {
            max_tables: 72,
            max_columns: 1024,
            max_functions: FUNCTION_CATALOG_LIMIT,
            max_operators: 128,
            large_table_row_threshold: 1_000_000,
            unknown_rows: UnknownRows::Refuse,
        };
        assert!(check_limits(&limits, 72).is_err());
        assert!(check_library_limits(&limits, 72).is_ok());
        limits.max_tables = 4097;
        assert!(check_library_limits(&limits, 72).is_err());
    }

    #[test]
    fn indexed_mappings_keep_identity_lookup_and_alias_guards() {
        let key = |column: &str| {
            (
                "source".to_owned(),
                "digest".to_owned(),
                1,
                "c".to_owned(),
                "schema".to_owned(),
                "table".to_owned(),
                column.to_owned(),
            )
        };
        let physical = format!("pc_{}", "a".repeat(60));
        let mut index = MappingIndex {
            rows: BTreeMap::new(),
        };
        index.insert(key("column"), (physical.clone(), true));
        let resolve = |index: &MappingIndex, source: &str, generation: i64| {
            index.resolve(
                source,
                "digest",
                generation,
                LEVEL_COLUMN,
                true,
                ("schema", "table", "column"),
            )
        };
        assert_eq!(resolve(&index, "source", 1).unwrap(), physical);
        assert!(matches!(
            resolve(&index, "other", 1),
            Err(CatalogError::MappingAbsent)
        ));
        assert!(matches!(
            resolve(&index, "source", 2),
            Err(CatalogError::MappingAbsent)
        ));
        let encoded = mapping_keys("column", true).pop().unwrap();
        index.insert(key(&encoded), (physical.clone(), true));
        assert!(matches!(
            resolve(&index, "source", 1),
            Err(CatalogError::AmbiguousMapping)
        ));
        index = MappingIndex {
            rows: BTreeMap::new(),
        };
        index.insert(key(&encoded), (physical.clone(), true));
        assert_eq!(resolve(&index, "source", 1).unwrap(), physical);
        index.insert(key(&encoded), (physical.clone(), false));
        assert!(matches!(
            resolve(&index, "source", 1),
            Err(CatalogError::AmbiguousMapping)
        ));
        index.insert(key(&encoded), (format!("pt_{}", "a".repeat(60)), true));
        assert!(matches!(
            resolve(&index, "source", 1),
            Err(CatalogError::AmbiguousMapping)
        ));
    }

    #[test]
    fn wide_library_mapping_lookups_preserve_all_scopes_without_flat_key_clones() {
        let mut index = MappingIndex {
            rows: BTreeMap::new(),
        };
        let digest = "d".repeat(64);
        for table in 0..658 {
            for column in 0..100 {
                let key = (
                    format!("source_{}", table % 47),
                    digest.clone(),
                    1,
                    "c".into(),
                    "namespace".into(),
                    format!("table_{table}"),
                    format!("column_{column}"),
                );
                assert!(
                    index
                        .insert(key, (format!("pc_{:060x}", table * 100 + column), true))
                        .is_none()
                );
            }
        }
        let started = std::time::Instant::now();
        for table in 0..658 {
            for column in 0..100 {
                assert_eq!(
                    index
                        .resolve(
                            &format!("source_{}", table % 47),
                            &digest,
                            1,
                            LEVEL_COLUMN,
                            true,
                            (
                                "namespace",
                                &format!("table_{table}"),
                                &format!("column_{column}")
                            )
                        )
                        .unwrap(),
                    format!("pc_{:060x}", table * 100 + column)
                );
            }
        }
        eprintln!(
            "wide_mapping_lookup tables=658 columns=65800 elapsed_ms={}",
            started.elapsed().as_millis()
        );
        let alias = mapping_keys("namespace", true).pop().unwrap();
        index.insert(
            (
                "source_0".into(),
                digest.clone(),
                1,
                "c".into(),
                alias,
                "table_0".into(),
                "column_0".into(),
            ),
            (format!("pc_{:060x}", 0), true),
        );
        assert_eq!(
            index.resolve(
                "source_0",
                &digest,
                1,
                LEVEL_COLUMN,
                true,
                ("namespace", "table_0", "column_0")
            ),
            Err(CatalogError::AmbiguousMapping)
        );
    }
}

fn verify_library_table_vector(
    bound: &BoundInput,
    table: &TableBound,
    entries: &BTreeMap<&str, &Value>,
    index: &MappingIndex,
    physical_index: &snapshot::Relations,
    proofs: &snapshot::LibraryProofs,
) -> Result<(), CatalogError> {
    let dataset = format!("{}.{}", table.namespace, table.name);
    let entry = entries
        .get(dataset.as_str())
        .ok_or(CatalogError::IdentityMismatch)?;
    let namespace = index.resolve(
        &bound.source,
        &bound.config_digest,
        bound.generation,
        LEVEL_NAMESPACE,
        bound.native_namespace,
        (&table.namespace, "", ""),
    )?;
    let physical = index.resolve(
        &bound.source,
        &bound.config_digest,
        bound.generation,
        LEVEL_TABLE,
        bound.native_namespace,
        (&table.namespace, &table.name, ""),
    )?;
    let relation = physical_index.get(&namespace, &physical)?;
    proofs.verify(&dataset)?;
    let expected = entry["expected_columns"]
        .as_array()
        .ok_or(CatalogError::IdentityMismatch)?;
    if expected.len() != table.columns.len() {
        return Err(CatalogError::IdentityMismatch);
    }
    let mut expected_types = BTreeMap::new();
    for column in expected {
        let name = column["name"]
            .as_str()
            .ok_or(CatalogError::IdentityMismatch)?;
        let ty = column["data_type"]
            .as_str()
            .ok_or(CatalogError::IdentityMismatch)?;
        if expected_types.insert(name, ty).is_some() {
            return Err(CatalogError::IdentityMismatch);
        }
    }
    let published = mapped_columns(index, bound, table)?;
    let actual_types: BTreeMap<_, _> = relation
        .attributes
        .iter()
        .map(|item| (item.attribute.name.as_str(), item.formatted.as_str()))
        .collect();
    for (column, mapped) in &published {
        let expected = expected_types
            .get(column.as_str())
            .copied()
            .ok_or(CatalogError::IdentityMismatch)?;
        if actual_types.get(mapped.as_str()).copied() != Some(expected) {
            return Err(CatalogError::IdentityMismatch);
        }
    }
    Ok(())
}
