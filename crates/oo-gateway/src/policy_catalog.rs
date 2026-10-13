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
use serde_json::Value;
use sha2::{Digest, Sha256};

use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, Error as PgError, IsolationLevel, Transaction};

use crate::sql_guard::{
    LiteralTypes, PolicyCatalog, PolicyIdentity, ResolvedFunction, ResolvedOperator, VisibleColumn,
    VisibleRelation,
};

use super::CompiledPolicy;

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

/// Comparison and equality tokens. Arithmetic and pattern operators are absent.
pub const COMPARISON_OPERATORS: &[&str] = &["<", "<=", "<>", "=", ">", ">="];

/// Same-type pairs, plus the int4 pairs an integer literal needs on either side.
///
/// Integer literals use the `int4` type OID. `int2` and `int8` columns compare
/// with that literal only through the cross-type builtin operators. `numeric`
/// columns compare with an integer literal through the `numeric`/`numeric`
/// operator; PostgreSQL casts the literal. A decimal-point literal is still
/// rejected before this list is consulted.
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
    load_policy_catalog_bound_source(client, identity, policies, limits, Some(source), None, None).await
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
    load_policy_catalog_bound_source(client, identity, policies, limits, Some(source), vector, catalog).await
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
    let mut bound = prepare(identity, policies, limits, vector.is_some())?;
    if let Some(source) = source {
        if !closed_identity(source) && !(vector.is_some() && source.is_empty()) {
            return Err(CatalogError::IdentityMismatch);
        }
        bound.source = source.to_owned();
        for table in &mut bound.tables { table.source = source.to_owned(); }
        bound.native_namespace = true;
    }
    if let Some(vector) = vector {
        bind_vector_sources(&mut bound, vector, catalog.ok_or(CatalogError::IdentityMismatch)?)?;
    }
    require_tls(client).await?;
    let tx = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .map_err(db_error)?;
    match load_in_snapshot(&tx, identity, &bound, limits, vector, catalog).await {
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

fn bind_vector_sources(bound: &mut BoundInput, vector: &Value, catalog: &Value) -> Result<(), CatalogError> {
    let entries = vector.as_array().ok_or(CatalogError::IdentityMismatch)?;
    if entries.len() != bound.tables.len() || catalog["entries"].as_array().is_none_or(|items| items.len() != entries.len()) {
        return Err(CatalogError::IdentityMismatch);
    }
    for table in &mut bound.tables {
        let dataset = format!("{}.{}", table.namespace, table.name);
        let mut matches = entries.iter().filter(|entry| entry["dataset"] == dataset);
        let entry = matches.next().ok_or(CatalogError::IdentityMismatch)?;
        if matches.next().is_some() { return Err(CatalogError::IdentityMismatch); }
        let table_source = entry["source_id"].as_str().ok_or(CatalogError::IdentityMismatch)?;
        if !closed_identity(table_source) { return Err(CatalogError::IdentityMismatch); }
        table.source = table_source.to_owned();
    }
    Ok(())
}

fn prepare(
    identity: &PolicyIdentity,
    policies: &[CompiledPolicy],
    limits: &CatalogLimits,
    native_vector: bool,
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
    vector: Option<&Value>,
    catalog: Option<&Value>,
) -> Result<PolicyCatalog, CatalogError> {
    tx.batch_execute("SET LOCAL search_path TO pg_catalog; SET LOCAL statement_timeout = '30s'")
        .await
        .map_err(db_error)?;
    ensure_snapshot(tx).await?;
    for table in &bound.tables {
        let scoped = BoundInput { source: table.source.clone(), config_digest: table.config_digest.clone(), generation: table.generation, ..bound.clone() };
        ensure_generation(tx, &scoped).await?;
    }
    ensure_role(tx, &bound.role).await?;
    let types = load_types(tx).await?;
    let operators = load_operators(tx, &types, limits).await?;
    let functions = load_functions(tx, &types, limits).await?;
    let mut relations = Vec::with_capacity(bound.tables.len());
    for table in &bound.tables {
        let scoped = BoundInput { source: table.source.clone(), config_digest: table.config_digest.clone(), generation: table.generation, ..bound.clone() };
        if let Some(vector) = vector {
            verify_native_table_vector(tx, &scoped, table, vector, catalog.ok_or(CatalogError::IdentityMismatch)?).await?;
        }
        if let Some(relation) = load_relation(tx, &scoped, table, &types, limits).await? {
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
            &[&bound.source, &bound.config_digest],
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

async fn verify_native_table_vector(
    tx: &Transaction<'_>,
    bound: &BoundInput,
    table: &TableBound,
    vector: &Value,
    catalog: &Value,
) -> Result<(), CatalogError> {
    let dataset = format!("{}.{}", table.namespace, table.name);
    let entries = vector.as_array().ok_or(CatalogError::IdentityMismatch)?;
    let entry = entries.iter().find(|item| item["dataset"] == dataset).ok_or(CatalogError::IdentityMismatch)?;
    let text = |key: &str| entry[key].as_str().ok_or(CatalogError::IdentityMismatch);
    if text("source_id")? != bound.source
        || text("config_digest")? != table.config_digest
        || entry["source_generation"].as_i64() != Some(table.generation)
    {
        return Err(CatalogError::IdentityMismatch);
    }
    let namespace = mapped_name(tx, bound, LEVEL_NAMESPACE, &table.namespace, "", "").await?;
    let physical = mapped_name(tx, bound, LEVEL_TABLE, &table.namespace, &table.name, "").await?;
    if namespace != text("stored_namespace")? || physical != text("stored_physical")? {
        return Err(CatalogError::IdentityMismatch);
    }
    let target_generation = entry["target_generation"].as_i64().ok_or(CatalogError::IdentityMismatch)?;
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
    if count != 1 { return Err(CatalogError::IdentityMismatch); }
    let catalog_entries = catalog["entries"].as_array().ok_or(CatalogError::IdentityMismatch)?;
    let matching = catalog_entries.iter().filter(|candidate| {
        candidate["run_id"] == entry["run_id"]
            && candidate["tables"][0]["logical_schema"] == table.namespace
            && candidate["tables"][0]["logical_table"] == table.name
    }).collect::<Vec<_>>();
    if matching.len() != 1 { return Err(CatalogError::IdentityMismatch); }
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
    { return Err(CatalogError::IdentityMismatch); }
    let columns = catalog_entry["tables"][0]["columns"].as_array().ok_or(CatalogError::IdentityMismatch)?;
    let relation = relation_row(tx, &namespace, &physical, true).await?;
    let rows = tx.query(
        "SELECT a.attname::text,a.atttypid::int8,a.atttypmod::int4 \
         FROM pg_catalog.pg_attribute a \
         WHERE a.attrelid=($1::int4)::oid \
           AND a.attnum>0 AND NOT a.attisdropped AND a.attname <> '_oo_pos' \
         ORDER BY a.attnum",
        &[&relation.oid],
    ).await.map_err(db_error)?;
    if rows.len() != columns.len() { return Err(CatalogError::IdentityMismatch); }
    let mut preimage = String::new();
    oo_core::sealed_import::push_schema_table(&mut preimage, &table.namespace, &table.name);
    for (row, column) in rows.iter().zip(columns) {
        let physical_name: String = row.get(0);
        let oid: i64 = row.get(1);
        let typmod: i32 = row.get(2);
        let logical_name = column["logical_name"].as_str().ok_or(CatalogError::IdentityMismatch)?;
        if column["stored_physical"] != physical_name
            || column["oid"].as_i64() != Some(oid)
            || column["typmod"].as_i64() != Some(i64::from(typmod))
        { return Err(CatalogError::IdentityMismatch); }
        let caveat = if column["caveat"].is_null() { None } else { Some(column["caveat"].as_str().ok_or(CatalogError::IdentityMismatch)?) };
        oo_core::sealed_import::push_schema_column(&mut preimage, logical_name, oid, typmod, caveat);
    }
    let calculated = format!("{:x}", Sha256::digest(preimage.as_bytes()));
    if catalog_entry["schema_digest"] != calculated { return Err(CatalogError::IdentityMismatch); }
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

async fn load_functions(
    tx: &Transaction<'_>,
    types: &BTreeMap<String, u32>,
    limits: &CatalogLimits,
) -> Result<Vec<ResolvedFunction>, CatalogError> {
    let mut functions = load_count(tx, types, limits).await?;
    for (name, type_name) in [
        ("sum", "numeric"),
        ("sum", "int8"),
        ("min", "int8"),
        ("max", "int8"),
    ] {
        let loaded = u32::try_from(functions.len()).map_err(|_| CatalogError::LimitExceeded)?;
        if loaded >= limits.max_functions {
            break;
        }
        functions.push(load_unary_aggregate(tx, types, name, type_name).await?);
    }
    Ok(functions)
}

async fn load_unary_aggregate(
    tx: &Transaction<'_>,
    types: &BTreeMap<String, u32>,
    name: &str,
    type_name: &str,
) -> Result<ResolvedFunction, CatalogError> {
    let type_oid = types.get(type_name).ok_or(CatalogError::MissingObject)?;
    let type_i = i32::try_from(*type_oid).map_err(|_| CatalogError::MissingObject)?;
    let function_name = name.to_string();
    let params: [&(dyn ToSql + Sync); 2] = [&function_name, &type_i];
    let count_row = tx
        .query_one(
            &format!("SELECT count(*)::int8 {UNARY_AGGREGATE_FROM}"),
            &params,
        )
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
            &format!("SELECT procedure.oid::int4, procedure.proname {UNARY_AGGREGATE_FROM}"),
            &params,
        )
        .await
        .map_err(db_error)?;
    let found_name: String = row.get(1);
    if found_name != name {
        return Err(CatalogError::MissingObject);
    }
    Ok(ResolvedFunction {
        oid: oid_from_i32(row.get(0))?,
        schema: "pg_catalog".to_string(),
        name: found_name,
        argument_type_oids: vec![*type_oid],
        aggregate_star: false,
    })
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

const UNARY_AGGREGATE_FROM: &str = "\
FROM pg_catalog.pg_proc AS procedure \
JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = procedure.pronamespace \
JOIN pg_catalog.pg_aggregate AS aggregate ON aggregate.aggfnoid = procedure.oid \
JOIN pg_catalog.pg_language AS language ON language.oid = procedure.prolang \
JOIN pg_catalog.pg_type AS argument ON argument.oid = procedure.proargtypes[0] \
WHERE namespace.nspname = 'pg_catalog' \
  AND procedure.proname = $1 \
  AND procedure.prokind = 'a' \
  AND procedure.pronargs = 1 \
  AND procedure.provolatile = 'i' \
  AND NOT procedure.proretset \
  AND argument.oid = ($2::int4)::oid \
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

async fn load_relation(
    tx: &Transaction<'_>,
    bound: &BoundInput,
    table: &TableBound,
    types: &BTreeMap<String, u32>,
    limits: &CatalogLimits,
) -> Result<Option<VisibleRelation>, CatalogError> {
    let physical_schema = if bound.native_namespace {
        mapped_name(tx, bound, LEVEL_NAMESPACE, &table.namespace, "", "").await?
    } else {
        PHYSICAL_SCHEMA.to_owned()
    };
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
    let relation = relation_row(
        tx,
        &physical_schema,
        &physical_table,
        bound.native_namespace,
    )
    .await?;
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
        physical_schema: relation.schema,
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

async fn relation_row(
    tx: &Transaction<'_>,
    schema: &str,
    physical: &str,
    native: bool,
) -> Result<RelationRow, CatalogError> {
    let count_row = tx
        .query_one(
            "SELECT count(*)::int8 \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE (namespace.nspname = $1 OR ($3 AND namespace.nspname = '_oo')) AND relation.relname = $2 AND relation.relkind = 'r'",
            &[&schema, &physical, &native],
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
            "SELECT relation.oid::int4, relation.reltuples, namespace.nspname \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE (namespace.nspname = $1 OR ($3 AND namespace.nspname = '_oo')) AND relation.relname = $2 AND relation.relkind = 'r'",
            &[&schema, &physical, &native],
        )
        .await
        .map_err(db_error)?;
    Ok(RelationRow {
        schema: row.get(2),
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

const LEVEL_NAMESPACE: MapLevel = MapLevel {
    word: "namespace",
    letter: "n",
    prefix: "pn_",
};
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
                &bound.source,
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
                &bound.source,
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
                &bound.source,
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

    #[test]
    fn native_vector_scopes_each_table_to_its_real_source() {
        let make_table = |name: &str| TableBound {
            source: "aggregate".into(), namespace: "sales".into(), name: name.into(),
            config_digest: "a".repeat(64), generation: 3, columns: vec!["id".into()],
        };
        let mut bound = BoundInput {
            source: "aggregate".into(), native_namespace: true, role: "oo_r_sales".into(),
            config_digest: "a".repeat(64), generation: 3,
            tables: vec![make_table("east"), make_table("west")],
        };
        let vector = serde_json::json!([
            {"dataset":"sales.east","source_id":"east_source"},
            {"dataset":"sales.west","source_id":"west_source"}
        ]);
        let catalog = serde_json::json!({"entries":[{},{}]});
        bind_vector_sources(&mut bound, &vector, &catalog).unwrap();
        assert_eq!(bound.tables.iter().map(|table| table.source.as_str()).collect::<Vec<_>>(), vec!["east_source", "west_source"]);
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
        assert_eq!(required_operator_count(), 60);
    }

    #[test]
    fn whitelist_covers_only_the_comparison_subset() {
        assert_eq!(required_operator_count(), 60);
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
            refused(prepare(&identity(), &[], &limits(), false)),
            CatalogError::EmptyInput
        );
        let mut wrong = identity();
        wrong.role = "oo_r_auditor".to_string();
        assert_eq!(
            refused(prepare(&wrong, std::slice::from_ref(&clerk), &limits(), false)),
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
