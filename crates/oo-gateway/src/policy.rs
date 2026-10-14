//! Column ACL compiler for mapped shadow tables.
//!
//! A trusted administrator connection installs a `NOLOGIN` role and column
//! `SELECT` grants. [`install_column_acl`] is one table.
//! [`install_column_acl_set`] is one role across a bounded list of tables in
//! one transaction. The caller supplies the connection. This module does not
//! accept a password, a raw `GRANT` string, or a row filter.
//!
//! Row filters are refused before any database call. PostgreSQL still allows
//! `SET` of a custom GUC after `REVOKE` of `set_config`, so a claims predicate
//! is not an enforceable proof.

use std::collections::BTreeSet;
use std::fmt;

use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;
use rustls::{ClientConfig, RootCertStore};
use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, Error as PgError, Transaction};

#[path = "policy_catalog.rs"]
pub mod catalog;

/// TLS stream produced by [`rustls_connector`].
pub type TlsStream =
    <tokio_postgres_rustls::MakeRustlsConnect as tokio_postgres::tls::MakeTlsConnect<
        tokio_postgres::Socket,
    >>::Stream;

/// Connection future for [`connect_require_tls`]. Poll it, or the session stalls.
pub type TlsConnection = tokio_postgres::Connection<tokio_postgres::Socket, TlsStream>;

/// Logical table bound to a stored physical mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicalTable {
    /// Mapping namespace. A lowercase identifier, not a physical name.
    pub namespace: String,
    /// Mapping table name.
    pub name: String,
    /// Columns the role may read, in grant order.
    pub columns: Vec<ColumnSpec>,
}

/// One allowlisted column and its classification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnSpec {
    /// Logical column name.
    pub name: String,
    /// Published classification state.
    pub status: ClassStatus,
    /// Non-empty semantic type. Not written into SQL.
    pub semantic_type: String,
    /// PII decision. Unknown PII is not granted.
    pub pii: PiiClass,
}

/// Classification required before a column grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassStatus {
    /// A reviewer confirmed the column.
    Confirmed,
    /// The published rule auto-accepted the column.
    AutoAccepted,
    /// Not published. The column stays invisible.
    Unclassified,
}

/// PII decision for one column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PiiClass {
    /// The column is not personal data.
    None,
    /// The column is personal data. `explicit_allow` must be true to grant it.
    Declared { explicit_allow: bool },
    /// Classification did not decide. The column stays invisible.
    Unknown,
}

/// A row predicate. Every variant is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowFilter {
    /// Claims comparison. Not installed.
    Claims { column: String, claim: String },
}

/// Versioned column policy. One role, one table, no row filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnAclPolicy {
    /// Physical-mapping source. This slice uses it as the project key.
    pub project: String,
    /// 64 lowercase hex digits. Must match `physical_mappings.config_digest`.
    pub config_digest: String,
    /// Mapping `run_generation`. Must equal the maximum stored for this
    /// source, config digest, and logical table. Another table's later
    /// generation does not raise it, and neither does another digest.
    pub generation: i64,
    /// Role slug. The database role is `oo_r_` plus this slug.
    pub role: String,
    /// Existing login that may `SET` the role. Not an authentication token.
    pub login: String,
    /// The only table this policy grants.
    pub table: LogicalTable,
    /// Any value refuses the whole policy before SQL runs.
    pub row_filter: Option<RowFilter>,
}

/// Policy that passed the pure checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledPolicy {
    project: String,
    config_digest: String,
    generation: i64,
    role: String,
    login: String,
    namespace: String,
    table: String,
    columns: Vec<ColumnSpec>,
}

/// What one successful install did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallEffect {
    /// The role was created in this transaction.
    Installed,
    /// The same role and column set were already present.
    Replay,
}

/// Result of a committed install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallOutcome {
    /// Whether the role was created now or already matched.
    pub effect: InstallEffect,
    /// `oo_r_<slug>`.
    pub role: String,
    /// `session_user` that issued the grants. Never `oo_target_owner`.
    pub grantor: String,
    /// Physical shadow table in `_oo`.
    pub physical_table: String,
    /// Physical columns granted, in policy order.
    pub physical_columns: Vec<String>,
}

/// Refusal or database failure. Display text has no password and no `GRANT` script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyError {
    /// The input contains a row filter. No SQL was sent.
    RowFilterRefused,
    /// An identifier, digest, or semantic type failed validation.
    InvalidIdentifier {
        /// Which field failed.
        field: &'static str,
    },
    /// A requested column is not confirmed, or its PII decision is incomplete.
    ClassificationRefused {
        /// Logical column name.
        column: String,
    },
    /// The allowlist is empty.
    EmptyAllowlist,
    /// The policy generation is not the stored maximum.
    StaleGeneration {
        /// Generation in the policy.
        policy: i64,
        /// Maximum `run_generation` stored for this logical table, if any.
        current: Option<i64>,
    },
    /// `session_user` is `oo_target_owner`, or `current_user` differs from it.
    GrantorRefused,
    /// `pg_stat_ssl.ssl` is not true for this backend.
    TlsRequired,
    /// The role exists and is not the `NOLOGIN` shape this installer creates.
    RoleMismatch,
    /// Column grants already on the role are not this policy.
    GrantMismatch,
    /// The login role is missing, or it is `oo_target_owner`.
    LoginAbsent,
    /// Lookup did not return the mapped object.
    MappingAbsent,
    /// The mapped table or column is not in `_oo`.
    PhysicalAbsent,
    /// The CA PEM did not yield a certificate.
    CertificateRejected,
    /// The aggregate policy list is empty. No SQL was sent.
    EmptyPolicySet,
    /// The list does not share one role, login, project, digest, and generation.
    MixedIdentity,
    /// More than 32 tables, or one table has more than 512 columns.
    LimitExceeded,
    /// A mapped column is not a supported base type.
    TypeRefused {
        /// Logical column name.
        column: String,
    },
    /// A metadata column was named, or `_oo_pos` is selectable.
    MetadataColumn {
        /// Logical name, or `_oo_pos` for the physical metadata column.
        column: String,
    },
    /// An existing role's membership is not exactly this login.
    MembershipMismatch,
    /// PostgreSQL rejected a statement. `message` is the driver text.
    Database {
        /// SQLSTATE, or empty when the driver has none.
        sqlstate: String,
        /// Driver message. Identifiers only; this path has no password.
        message: String,
    },
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RowFilterRefused => formatter.write_str("row filter is refused before any grant"),
            Self::InvalidIdentifier { field } => {
                write!(formatter, "invalid policy identifier: {field}")
            }
            Self::ClassificationRefused { column } => {
                write!(formatter, "column is not grantable: {column}")
            }
            Self::EmptyAllowlist => formatter.write_str("column allowlist is empty"),
            Self::StaleGeneration { policy, current } => {
                write!(
                    formatter,
                    "policy generation {policy} is not the stored generation {current:?}"
                )
            }
            Self::GrantorRefused => {
                formatter.write_str("policy install grantor must not be oo_target_owner")
            }
            Self::TlsRequired => formatter.write_str("policy install requires a TLS session"),
            Self::RoleMismatch => formatter.write_str("existing role attributes do not match"),
            Self::GrantMismatch => formatter.write_str("existing column grants do not match"),
            Self::LoginAbsent => formatter.write_str("login role is absent or not usable"),
            Self::MappingAbsent => formatter.write_str("stored physical mapping is absent"),
            Self::PhysicalAbsent => formatter.write_str("mapped relation is absent in _oo"),
            Self::CertificateRejected => formatter.write_str("CA certificate was rejected"),
            Self::EmptyPolicySet => formatter.write_str("column policy set is empty"),
            Self::MixedIdentity => formatter
                .write_str("column policy set mixes role, login, project, digest, or generation"),
            Self::LimitExceeded => formatter.write_str("column policy set exceeds a bound"),
            Self::TypeRefused { column } => {
                write!(formatter, "mapped column type is not grantable: {column}")
            }
            Self::MetadataColumn { column } => {
                write!(formatter, "metadata column is not grantable: {column}")
            }
            Self::MembershipMismatch => {
                formatter.write_str("existing role membership does not match")
            }
            Self::Database { sqlstate, message } => {
                write!(formatter, "postgres sqlstate={sqlstate} {message}")
            }
        }
    }
}

impl std::error::Error for PolicyError {}

/// Pure compile. A row filter returns [`PolicyError::RowFilterRefused`] and
/// does not need a connection.
pub fn compile_column_acl(policy: &ColumnAclPolicy) -> Result<CompiledPolicy, PolicyError> {
    compile_column_acl_with_native_identifiers(policy, false)
}

pub fn compile_column_acl_native(
    policy: &ColumnAclPolicy,
) -> Result<CompiledPolicy, PolicyError> {
    compile_column_acl_with_native_identifiers(policy, true)
}

fn compile_column_acl_with_native_identifiers(
    policy: &ColumnAclPolicy,
    native: bool,
) -> Result<CompiledPolicy, PolicyError> {
    if policy.row_filter.is_some() {
        return Err(PolicyError::RowFilterRefused);
    }
    let project = require_source(&policy.project, "project")?;
    let config_digest = require_digest(&policy.config_digest)?;
    if policy.generation < 1 {
        return Err(PolicyError::InvalidIdentifier {
            field: "generation",
        });
    }
    let role_slug = require_role_slug(&policy.role)?;
    let role = format!("oo_r_{role_slug}");
    let login = require_login(&policy.login, &role)?;
    let namespace = require_logical(&policy.table.namespace, "namespace")?;
    let table = require_table_ident(&policy.table.name, "table", native)?;
    if policy.table.columns.is_empty() {
        return Err(PolicyError::EmptyAllowlist);
    }
    let mut seen = BTreeSet::new();
    let mut columns = Vec::with_capacity(policy.table.columns.len());
    for column in &policy.table.columns {
        let name = require_table_ident(&column.name, "column", native)?;
        if !seen.insert(name.clone()) {
            return Err(PolicyError::InvalidIdentifier { field: "column" });
        }
        if !classification_ok(column) {
            return Err(PolicyError::ClassificationRefused { column: name });
        }
        columns.push(ColumnSpec {
            name,
            status: column.status,
            semantic_type: column.semantic_type.clone(),
            pii: column.pii,
        });
    }
    Ok(CompiledPolicy {
        project,
        config_digest,
        generation: policy.generation,
        role,
        login,
        namespace,
        table,
        columns,
    })
}

/// Where a TLS administrator connects. The password stays with the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlsEndpoint {
    /// DNS name checked against the server certificate.
    pub host: String,
    /// PostgreSQL port.
    pub port: u16,
    /// Database name.
    pub database: String,
    /// Session user. Not `oo_target_owner`.
    pub user: String,
}

/// rustls connector that verifies `ca_pem` and the server name.
///
/// The crypto provider is aws-lc-rs, the provider already pinned in this
/// workspace. There is no `NoTls` constructor and no certificate skip flag.
pub fn rustls_connector(
    ca_pem: &[u8],
) -> Result<tokio_postgres_rustls::MakeRustlsConnect, PolicyError> {
    let certificate =
        CertificateDer::from_pem_slice(ca_pem).map_err(|_| PolicyError::CertificateRejected)?;
    let mut roots = RootCertStore::empty();
    roots
        .add(certificate)
        .map_err(|_| PolicyError::CertificateRejected)?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let config = ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|_| PolicyError::CertificateRejected)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(tokio_postgres_rustls::MakeRustlsConnect::new(config))
}

/// Open a session that requires TLS and verifies the certificate.
///
/// `password` is passed to the driver and is not copied into errors.
pub async fn connect_require_tls(
    endpoint: &TlsEndpoint,
    password: &str,
    ca_pem: &[u8],
) -> Result<(Client, TlsConnection), PolicyError> {
    if endpoint.host.is_empty() || endpoint.database.is_empty() || endpoint.user.is_empty() {
        return Err(PolicyError::InvalidIdentifier { field: "endpoint" });
    }
    let tls = rustls_connector(ca_pem)?;
    let mut config = tokio_postgres::Config::new();
    config.user(&endpoint.user);
    config.password(password);
    config.host(&endpoint.host);
    config.port(endpoint.port);
    config.dbname(&endpoint.database);
    config.ssl_mode(tokio_postgres::config::SslMode::Require);
    config.application_name("oo-gateway-policy");
    config.connect(tls).await.map_err(db_error)
}

/// Maximum tables in one [`install_column_acl_set`] call.
pub const COLUMN_ACL_SET_MAX_TABLES: usize = 32;

/// Maximum columns on one table in that call. Matches the catalog column bound.
pub const COLUMN_ACL_SET_MAX_COLUMNS: usize = 512;

/// Policies that passed the pure aggregate checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledPolicySet {
    policies: Vec<CompiledPolicy>,
}

impl CompiledPolicySet {
    /// Number of tables in the set.
    pub fn len(&self) -> usize {
        self.policies.len()
    }

    /// True when the set has no tables. [`compile_column_acl_set`] does not
    /// return that value.
    pub fn is_empty(&self) -> bool {
        self.policies.is_empty()
    }

    /// Database role name, `oo_r_` plus the shared slug.
    pub fn role(&self) -> &str {
        &self.policies[0].role
    }
}

/// One table in a committed aggregate install. Order follows the input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetTableGrant {
    /// Physical shadow table in `_oo`.
    pub physical_table: String,
    /// Physical columns granted, in policy order.
    pub physical_columns: Vec<String>,
}

/// Result of a committed aggregate install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetInstallOutcome {
    /// Created now, or an exact full-set replay.
    pub effect: InstallEffect,
    /// `oo_r_<slug>`.
    pub role: String,
    /// `session_user` that issued the grants. Never `oo_target_owner`.
    pub grantor: String,
    /// One entry per input policy, in that order.
    pub tables: Vec<SetTableGrant>,
}

/// Pure aggregate compile. Every row filter, identifier, classification, and
/// bound is checked before a connection is used.
pub fn compile_column_acl_set(
    policies: &[ColumnAclPolicy],
) -> Result<CompiledPolicySet, PolicyError> {
    compile_set(policies, false)
}

/// Compile one native source whose sealed collection binds each table independently.
/// The shared project, role and login remain identical; each table retains its own mapping seal.
pub fn compile_column_acl_native_vector_set(
    policies: &[ColumnAclPolicy],
) -> Result<CompiledPolicySet, PolicyError> {
    compile_set(policies, true)
}

fn compile_set(policies: &[ColumnAclPolicy], native_vector: bool) -> Result<CompiledPolicySet, PolicyError> {
    if policies.is_empty() {
        return Err(PolicyError::EmptyPolicySet);
    }
    if policies.len() > COLUMN_ACL_SET_MAX_TABLES {
        return Err(PolicyError::LimitExceeded);
    }
    let mut compiled = Vec::with_capacity(policies.len());
    for policy in policies {
        let one = compile_column_acl_with_native_identifiers(policy, native_vector)?;
        if one.columns.len() > COLUMN_ACL_SET_MAX_COLUMNS {
            return Err(PolicyError::LimitExceeded);
        }
        for column in &one.columns {
            if metadata_column(&column.name) {
                return Err(PolicyError::MetadataColumn {
                    column: column.name.clone(),
                });
            }
        }
        compiled.push(one);
    }
    let first = &compiled[0];
    let mut tables = BTreeSet::new();
    for policy in &compiled {
        if policy.project != first.project
            || (!native_vector && policy.config_digest != first.config_digest)
            || (!native_vector && policy.generation != first.generation)
            || policy.role != first.role
            || policy.login != first.login
        {
            return Err(PolicyError::MixedIdentity);
        }
        if !tables.insert((policy.namespace.clone(), policy.table.clone())) {
            return Err(PolicyError::InvalidIdentifier { field: "table" });
        }
    }
    Ok(CompiledPolicySet { policies: compiled })
}

/// Install one role for every table in `policies`.
///
/// Compile, mapping, type, generation, PII, and row-filter checks finish
/// before `CREATE ROLE` or any `GRANT`. A new role is created once. An
/// existing role is replayed only when its membership and its full column
/// grant set already equal this list. A partial list, a foreign table, or a
/// changed column is [`PolicyError::GrantMismatch`] and does not revoke or
/// add grants. Any error rolls the transaction back, including a role created
/// in that transaction.
pub async fn install_column_acl_set(
    client: &mut Client,
    policies: &[ColumnAclPolicy],
) -> Result<SetInstallOutcome, PolicyError> {
    let compiled = compile_column_acl_set(policies)?;
    require_tls_session(client).await?;
    let mut tx = client.transaction().await.map_err(db_error)?;
    match install_set_in_transaction(&mut tx, &compiled).await {
        Ok(outcome) => {
            tx.commit().await.map_err(db_error)?;
            Ok(outcome)
        }
        Err(err) => {
            let _ = tx.rollback().await;
            Err(err)
        }
    }
}

/// Install the exact per-table native vector for a separate preview role.
/// Each table is resolved and generation-checked in the same transaction.
pub async fn install_column_acl_native_vector_set(
    client: &mut Client,
    policies: &[ColumnAclPolicy],
) -> Result<SetInstallOutcome, PolicyError> {
    let compiled = compile_column_acl_native_vector_set(policies)?;
    require_tls_session(client).await?;
    let mut tx = client.transaction().await.map_err(db_error)?;
    match install_set_in_transaction(&mut tx, &compiled).await {
        Ok(outcome) => { tx.commit().await.map_err(db_error)?; Ok(outcome) }
        Err(err) => { let _ = tx.rollback().await; Err(err) }
    }
}

/// Validate an operator-authorized previous native policy set. Only generations may advance.
pub fn compile_column_acl_native_transition(
    previous: &[ColumnAclPolicy], current: &[ColumnAclPolicy],
) -> Result<(), PolicyError> {
    let before = compile_column_acl_native_vector_set(previous)?;
    let after = compile_column_acl_native_vector_set(current)?;
    if before.policies.len() != after.policies.len() { return Err(PolicyError::MixedIdentity); }
    let mut advanced = false;
    for old in &before.policies {
        let new = after.policies.iter().find(|p| p.namespace == old.namespace && p.table == old.table)
            .ok_or(PolicyError::MixedIdentity)?;
        let mut equal = new.clone(); equal.generation = old.generation;
        if equal != *old || new.generation < old.generation { return Err(PolicyError::MixedIdentity); }
        advanced |= new.generation > old.generation;
    }
    if !advanced { return Err(PolicyError::MixedIdentity); }
    Ok(())
}

/// Atomically replace the exact previous grants with the latest generation's grants.
/// The previous policy bytes must be independently pinned by the operator caller.
/// Default installers retain their strict full-set replay contract.
pub async fn transition_column_acl_native_vector_set(
    client: &mut Client, previous: &[ColumnAclPolicy], current: &[ColumnAclPolicy],
) -> Result<SetInstallOutcome, PolicyError> {
    compile_column_acl_native_transition(previous, current)?;
    let before = compile_column_acl_native_vector_set(previous)?;
    let after = compile_column_acl_native_vector_set(current)?;
    require_tls_session(client).await?;
    let mut tx = client.transaction().await.map_err(db_error)?;
    match transition_set_in_transaction(&mut tx, &before, &after).await {
        Ok(outcome) => {
            tx.commit().await.map_err(db_error)?;
            Ok(outcome)
        }
        Err(err) => {
            let _ = tx.rollback().await;
            Err(err)
        }
    }
}

async fn transition_set_in_transaction(
    tx: &mut Transaction<'_>, before: &CompiledPolicySet, after: &CompiledPolicySet,
) -> Result<SetInstallOutcome, PolicyError> {
    let grantor = require_grantor(tx).await?;
    let first = &after.policies[0];
    tx.query_one("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))", &[&first.role]).await.map_err(db_error)?;
    let old = resolve_set(tx, before, false).await?;
    let new = resolve_set(tx, after, true).await?;
    // Physical source types must match too; a policy label cannot conceal a type change.
    for (index, policy) in after.policies.iter().enumerate() {
        let old_index = before.policies.iter().position(|p| p.namespace == policy.namespace && p.table == policy.table).ok_or(PolicyError::MixedIdentity)?;
        for (column_index, column) in policy.columns.iter().enumerate() {
            let (old_table, old_column) = (&old[old_index].physical_table, &old[old_index].physical_columns[column_index]);
            let (new_table, new_column) = (&new[index].physical_table, &new[index].physical_columns[column_index]);
            let same: bool = tx.query_one("SELECT (SELECT ROW(a.atttypid,a.atttypmod,a.attcollation)::text FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid=a.attrelid WHERE c.relnamespace='_oo'::regnamespace AND c.relname=$1 AND a.attname=$2 AND NOT a.attisdropped) IS NOT DISTINCT FROM (SELECT ROW(a.atttypid,a.atttypmod,a.attcollation)::text FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid=a.attrelid WHERE c.relnamespace='_oo'::regnamespace AND c.relname=$3 AND a.attname=$4 AND NOT a.attisdropped)", &[old_table, old_column, new_table, new_column]).await.map_err(db_error)?.get(0);
            if !same { return Err(PolicyError::TypeRefused { column: column.name.clone() }); }
        }
    }
    ensure_login(tx, &first.login).await?;
    if !role_exists(tx, &first.role).await? { return Err(PolicyError::RoleMismatch); }
    ensure_role_shape(tx, &first.role).await?;
    ensure_members_preserved(tx, &first.role, &first.login).await?;
    ensure_schema_usage(tx, &first.role).await?;
    ensure_no_future_select(tx, &first.role).await?;
    revoke_older_same_table_grants(tx, &first.role, before).await?;
    let effect = match ensure_exact_grants(tx, &first.role, &new).await {
        Ok(()) => InstallEffect::Replay,
        Err(PolicyError::GrantMismatch) => {
            ensure_exact_grants(tx, &first.role, &old).await?;
            let role = quote_ident(&first.role)?;
            for table in &old {
                let table_name = require_physical(&table.physical_table, "pt_")?;
                let columns = table.physical_columns.iter().map(|c| require_physical(c, "pc_")).collect::<Result<Vec<_>,_>>()?.join(", ");
                tx.batch_execute(&format!("REVOKE SELECT ({columns}) ON TABLE \"_oo\".{table_name} FROM {role}")).await.map_err(db_error)?;
            }
            for table in &new { grant_usage_and_columns(tx, &first.role, &table.physical_table, &table.physical_columns).await?; }
            ensure_exact_grants(tx, &first.role, &new).await?;
            InstallEffect::Installed
        }
        Err(error) => return Err(error),
    };
    Ok(SetInstallOutcome { effect, role: first.role.clone(), grantor,
        tables: new.into_iter().map(|t| SetTableGrant { physical_table:t.physical_table, physical_columns:t.physical_columns }).collect() })
}

struct ResolvedTable {
    physical_table: String,
    physical_columns: Vec<String>,
}

async fn resolve_set(
    tx: &mut Transaction<'_>, compiled: &CompiledPolicySet, require_current: bool,
) -> Result<Vec<ResolvedTable>, PolicyError> {
    let mut resolved = Vec::with_capacity(compiled.policies.len());
    let mut physical_tables = BTreeSet::new();
    for policy in &compiled.policies {
        let current = current_generation(tx, &policy.project, &policy.config_digest, &policy.namespace, &policy.table).await?;
        if require_current && current != Some(policy.generation) {
            return Err(PolicyError::StaleGeneration { policy: policy.generation, current });
        }
        let physical_table = lookup_relation(
            tx,
            policy,
            "table",
            &policy.namespace,
            &policy.table,
            "",
            "pt_",
        )
        .await?;
        if !physical_tables.insert(physical_table.clone()) {
            return Err(PolicyError::InvalidIdentifier { field: "table" });
        }
        let mut physical_columns = Vec::with_capacity(policy.columns.len());
        let mut seen_columns = BTreeSet::new();
        for column in &policy.columns {
            let physical = lookup_relation(
                tx,
                policy,
                "column",
                &policy.namespace,
                &policy.table,
                &column.name,
                "pc_",
            )
            .await?;
            if !seen_columns.insert(physical.clone()) {
                return Err(PolicyError::InvalidIdentifier { field: "column" });
            }
            ensure_grantable_type(tx, &physical_table, &physical, &column.name).await?;
            physical_columns.push(physical);
        }
        resolved.push(ResolvedTable {
            physical_table,
            physical_columns,
        });
    }
    Ok(resolved)
}

async fn install_set_in_transaction(
    tx: &mut Transaction<'_>,
    compiled: &CompiledPolicySet,
) -> Result<SetInstallOutcome, PolicyError> {
    let grantor = require_grantor(tx).await?;
    let first = &compiled.policies[0];
    let resolved = resolve_set(tx, compiled, true).await?;
    ensure_login(tx, &first.login).await?;
    let existed = role_exists(tx, &first.role).await?;
    if existed {
        ensure_role_shape(tx, &first.role).await?;
        ensure_membership(tx, &first.role, &first.login).await?;
        ensure_exact_grants(tx, &first.role, &resolved).await?;
        ensure_schema_usage(tx, &first.role).await?;
    }
    if !existed {
        create_role(tx, &first.role).await?;
    }
    grant_membership(tx, &first.role, &first.login).await?;
    if !existed {
        for table in &resolved {
            grant_usage_and_columns(
                tx,
                &first.role,
                &table.physical_table,
                &table.physical_columns,
            )
            .await?;
        }
        ensure_role_shape(tx, &first.role).await?;
        ensure_membership(tx, &first.role, &first.login).await?;
        ensure_exact_grants(tx, &first.role, &resolved).await?;
        ensure_schema_usage(tx, &first.role).await?;
    }
    Ok(SetInstallOutcome {
        effect: if existed {
            InstallEffect::Replay
        } else {
            InstallEffect::Installed
        },
        role: first.role.clone(),
        grantor,
        tables: resolved
            .into_iter()
            .map(|table| SetTableGrant {
                physical_table: table.physical_table,
                physical_columns: table.physical_columns,
            })
            .collect(),
    })
}

fn metadata_column(name: &str) -> bool {
    name == "_oo_pos" || name.starts_with("_oo_")
}

/// Install the compiled column ACL on `client`.
///
/// The session user must already be the grantor. This function does not
/// `SET ROLE`, does not read a password, and does not emit a grant for `PUBLIC`.
pub async fn install_column_acl(
    client: &mut Client,
    policy: &ColumnAclPolicy,
) -> Result<InstallOutcome, PolicyError> {
    let compiled = compile_column_acl(policy)?;
    require_tls_session(client).await?;
    let mut tx = client.transaction().await.map_err(db_error)?;
    match install_in_transaction(&mut tx, &compiled).await {
        Ok(outcome) => {
            tx.commit().await.map_err(db_error)?;
            Ok(outcome)
        }
        Err(err) => {
            let _ = tx.rollback().await;
            Err(err)
        }
    }
}

async fn install_in_transaction(
    tx: &mut Transaction<'_>,
    policy: &CompiledPolicy,
) -> Result<InstallOutcome, PolicyError> {
    let grantor = require_grantor(tx).await?;
    let current = current_generation(tx, &policy.project, &policy.config_digest, &policy.namespace, &policy.table).await?;
    if current != Some(policy.generation) {
        return Err(PolicyError::StaleGeneration {
            policy: policy.generation,
            current,
        });
    }
    let physical_table = lookup_relation(
        tx,
        policy,
        "table",
        &policy.namespace,
        &policy.table,
        "",
        "pt_",
    )
    .await?;
    let mut physical_columns = Vec::with_capacity(policy.columns.len());
    for column in &policy.columns {
        let physical = lookup_relation(
            tx,
            policy,
            "column",
            &policy.namespace,
            &policy.table,
            &column.name,
            "pc_",
        )
        .await?;
        ensure_column(tx, &physical_table, &physical).await?;
        physical_columns.push(physical);
    }
    let existed = role_exists(tx, &policy.role).await?;
    if existed {
        ensure_role_shape(tx, &policy.role).await?;
        let granted = granted_columns(tx, &policy.role, &physical_table).await?;
        if granted != physical_columns.iter().cloned().collect::<BTreeSet<_>>() {
            return Err(PolicyError::GrantMismatch);
        }
        ensure_no_table_privilege(tx, &policy.role, &physical_table).await?;
    } else {
        create_role(tx, &policy.role).await?;
    }
    grant_membership(tx, &policy.role, &policy.login).await?;
    if !existed {
        grant_usage_and_columns(tx, &policy.role, &physical_table, &physical_columns).await?;
        let granted = granted_columns(tx, &policy.role, &physical_table).await?;
        if granted != physical_columns.iter().cloned().collect::<BTreeSet<_>>() {
            return Err(PolicyError::GrantMismatch);
        }
        ensure_no_table_privilege(tx, &policy.role, &physical_table).await?;
    }
    Ok(InstallOutcome {
        effect: if existed {
            InstallEffect::Replay
        } else {
            InstallEffect::Installed
        },
        role: policy.role.clone(),
        grantor,
        physical_table,
        physical_columns,
    })
}

fn classification_ok(column: &ColumnSpec) -> bool {
    let published = matches!(
        column.status,
        ClassStatus::Confirmed | ClassStatus::AutoAccepted
    );
    let semantic = !column.semantic_type.is_empty()
        && column.semantic_type.len() <= 64
        && !column.semantic_type.chars().any(char::is_control);
    let pii = match column.pii {
        PiiClass::None => true,
        PiiClass::Declared { explicit_allow } => explicit_allow,
        PiiClass::Unknown => false,
    };
    published && semantic && pii
}

fn require_source(value: &str, field: &'static str) -> Result<String, PolicyError> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 || !bytes[0].is_ascii_lowercase() {
        return Err(PolicyError::InvalidIdentifier { field });
    }
    if !bytes[1..]
        .iter()
        .all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err(PolicyError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

fn require_logical(value: &str, field: &'static str) -> Result<String, PolicyError> {
    if !is_plain_ident(value) {
        return Err(PolicyError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

fn require_table_ident(
    value: &str,
    field: &'static str,
    native: bool,
) -> Result<String, PolicyError> {
    if !(native && is_native_ident(value) || !native && is_plain_ident(value)) {
        return Err(PolicyError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

fn is_native_ident(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && (bytes[0] == b'_' || bytes[0].is_ascii_alphabetic())
        && bytes[1..]
            .iter()
            .all(|byte| *byte == b'_' || byte.is_ascii_alphanumeric())
}

fn require_digest(value: &str) -> Result<String, PolicyError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(value.to_string())
    } else {
        Err(PolicyError::InvalidIdentifier {
            field: "config_digest",
        })
    }
}

fn require_role_slug(value: &str) -> Result<String, PolicyError> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 58 || !bytes[0].is_ascii_lowercase() {
        return Err(PolicyError::InvalidIdentifier { field: "role" });
    }
    if !bytes[1..]
        .iter()
        .all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err(PolicyError::InvalidIdentifier { field: "role" });
    }
    let role = format!("oo_r_{value}");
    if reserved_role(&role) {
        return Err(PolicyError::InvalidIdentifier { field: "role" });
    }
    Ok(value.to_string())
}

fn require_login(value: &str, role: &str) -> Result<String, PolicyError> {
    if !is_plain_ident(value) || value == role || reserved_role(value) {
        return Err(PolicyError::InvalidIdentifier { field: "login" });
    }
    Ok(value.to_string())
}

fn is_plain_ident(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let first = bytes[0];
    if first != b'_' && !first.is_ascii_lowercase() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn reserved_role(value: &str) -> bool {
    matches!(
        value,
        "public"
            | "postgres"
            | "oo_target_owner"
            | "oo_pipeline_runtime"
            | "oo_fixture_admin"
            | "oo_gateway"
            | "pg_read_all_stats"
    ) || value.starts_with("pg_")
}

fn quote_ident(value: &str) -> Result<String, PolicyError> {
    if !is_plain_ident(value) || value.contains('"') {
        return Err(PolicyError::InvalidIdentifier {
            field: "identifier",
        });
    }
    Ok(format!("\"{value}\""))
}

fn require_physical(value: &str, prefix: &str) -> Result<String, PolicyError> {
    let rest = value
        .strip_prefix(prefix)
        .ok_or(PolicyError::PhysicalAbsent)?;
    if value.len() == 63
        && rest.len() == 60
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        quote_ident(value)
    } else {
        Err(PolicyError::PhysicalAbsent)
    }
}

fn db_error(err: PgError) -> PolicyError {
    let sqlstate = err
        .code()
        .map(|code| code.code().to_string())
        .unwrap_or_default();
    if sqlstate == "OO024" {
        return PolicyError::MappingAbsent;
    }
    PolicyError::Database {
        sqlstate,
        message: err.to_string(),
    }
}

async fn require_tls_session(client: &mut Client) -> Result<(), PolicyError> {
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
        Err(PolicyError::TlsRequired)
    }
}

async fn require_grantor(tx: &mut Transaction<'_>) -> Result<String, PolicyError> {
    let row = tx
        .query_one("SELECT session_user::text, current_user::text", &[])
        .await
        .map_err(db_error)?;
    let session_user: String = row.get(0);
    let current_user: String = row.get(1);
    if session_user == current_user && session_user != "oo_target_owner" {
        Ok(session_user)
    } else {
        Err(PolicyError::GrantorRefused)
    }
}

async fn current_generation(
    tx: &mut Transaction<'_>,
    project: &str,
    config_digest: &str,
    namespace: &str,
    table: &str,
) -> Result<Option<i64>, PolicyError> {
    let row = tx
        .query_one(
            "SELECT max(run_generation) FROM _oo.physical_mappings \
             WHERE source = $1 AND config_digest = $2 AND level = 't' \
               AND logical_namespace = $3 AND logical_table = $4",
            &[&project, &config_digest, &namespace, &table],
        )
        .await
        .map_err(db_error)?;
    Ok(row.get(0))
}

async fn lookup_relation(
    tx: &mut Transaction<'_>,
    policy: &CompiledPolicy,
    level: &str,
    namespace: &str,
    table: &str,
    column: &str,
    prefix: &str,
) -> Result<String, PolicyError> {
    let level_word = level.to_string();
    let namespace_name = namespace.to_string();
    let table_name = table.to_string();
    let column_name = column.to_string();
    let params: [&(dyn ToSql + Sync); 7] = [
        &policy.project,
        &policy.config_digest,
        &policy.generation,
        &level_word,
        &namespace_name,
        &table_name,
        &column_name,
    ];
    let row = tx
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &params,
        )
        .await
        .map_err(db_error)?;
    let name: String = row.get(0);
    require_physical(&name, prefix)?;
    Ok(name)
}

async fn ensure_column(
    tx: &mut Transaction<'_>,
    table: &str,
    column: &str,
) -> Result<(), PolicyError> {
    let row = tx
        .query_opt(
            "SELECT 1 FROM pg_catalog.pg_attribute AS attribute \
             JOIN pg_catalog.pg_class AS relation ON relation.oid = attribute.attrelid \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname = $1 AND attribute.attname = $2 \
               AND attribute.attnum > 0 AND NOT attribute.attisdropped",
            &[&table, &column],
        )
        .await
        .map_err(db_error)?;
    if row.is_some() {
        Ok(())
    } else {
        Err(PolicyError::PhysicalAbsent)
    }
}

async fn role_exists(tx: &mut Transaction<'_>, role: &str) -> Result<bool, PolicyError> {
    let row = tx
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = $1)",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    Ok(row.get(0))
}

async fn ensure_role_shape(tx: &mut Transaction<'_>, role: &str) -> Result<(), PolicyError> {
    let row = tx
        .query_one(
            "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, rolbypassrls, rolreplication \
             FROM pg_catalog.pg_roles WHERE rolname = $1",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    let flags: [bool; 7] = [
        row.get(0),
        row.get(1),
        row.get(2),
        row.get(3),
        row.get(4),
        row.get(5),
        row.get(6),
    ];
    if flags.iter().any(|flag| *flag) {
        Err(PolicyError::RoleMismatch)
    } else {
        Ok(())
    }
}

async fn create_role(tx: &mut Transaction<'_>, role: &str) -> Result<(), PolicyError> {
    let quoted = quote_ident(role)?;
    tx.batch_execute(&format!(
        "CREATE ROLE {quoted} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION"
    ))
    .await
    .map_err(db_error)?;
    ensure_role_shape(tx, role).await
}

async fn ensure_login(tx: &mut Transaction<'_>, login: &str) -> Result<(), PolicyError> {
    let row = tx
        .query_opt(
            "SELECT rolsuper FROM pg_catalog.pg_roles WHERE rolname = $1",
            &[&login],
        )
        .await
        .map_err(db_error)?;
    let Some(row) = row else {
        return Err(PolicyError::LoginAbsent);
    };
    let superuser: bool = row.get(0);
    if superuser {
        Err(PolicyError::LoginAbsent)
    } else {
        Ok(())
    }
}

async fn grant_membership(
    tx: &mut Transaction<'_>,
    role: &str,
    login: &str,
) -> Result<(), PolicyError> {
    ensure_login(tx, login).await?;
    let quoted_role = quote_ident(role)?;
    let quoted_login = quote_ident(login)?;
    tx.batch_execute(&format!(
        "GRANT {quoted_role} TO {quoted_login} WITH INHERIT FALSE, SET TRUE"
    ))
    .await
    .map_err(db_error)
}

async fn grant_usage_and_columns(
    tx: &mut Transaction<'_>,
    role: &str,
    table: &str,
    columns: &[String],
) -> Result<(), PolicyError> {
    let quoted_role = quote_ident(role)?;
    let quoted_table = require_physical(table, "pt_")?;
    let mut quoted_columns = Vec::with_capacity(columns.len());
    for column in columns {
        quoted_columns.push(require_physical(column, "pc_")?);
    }
    let list = quoted_columns.join(", ");
    tx.batch_execute(&format!(
        "GRANT USAGE ON SCHEMA \"_oo\" TO {quoted_role};\n\
         GRANT SELECT ({list}) ON TABLE \"_oo\".{quoted_table} TO {quoted_role}"
    ))
    .await
    .map_err(db_error)
}

async fn granted_columns(
    tx: &mut Transaction<'_>,
    role: &str,
    table: &str,
) -> Result<BTreeSet<String>, PolicyError> {
    let rows = tx
        .query(
            "SELECT column_name::text FROM information_schema.column_privileges \
             WHERE grantee = $1 AND table_schema = '_oo' AND table_name = $2 AND privilege_type = 'SELECT'",
            &[&role, &table],
        )
        .await
        .map_err(db_error)?;
    let mut names = BTreeSet::new();
    for row in rows {
        names.insert(row.get(0));
    }
    Ok(names)
}

async fn ensure_no_table_privilege(
    tx: &mut Transaction<'_>,
    role: &str,
    table: &str,
) -> Result<(), PolicyError> {
    let row = tx
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.table_privileges \
             WHERE grantee = $1 AND table_schema = '_oo' AND table_name = $2",
            &[&role, &table],
        )
        .await
        .map_err(db_error)?;
    let count: i64 = row.get(0);
    if count == 0 {
        Ok(())
    } else {
        Err(PolicyError::GrantMismatch)
    }
}

async fn ensure_grantable_type(
    tx: &mut Transaction<'_>,
    table: &str,
    column: &str,
    logical: &str,
) -> Result<(), PolicyError> {
    let row = tx
        .query_opt(
            "SELECT type_row.typname::text, type_row.typtype::text, attribute.atttypmod::int4 \
             FROM pg_catalog.pg_attribute AS attribute \
             JOIN pg_catalog.pg_class AS relation ON relation.oid = attribute.attrelid \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             JOIN pg_catalog.pg_type AS type_row ON type_row.oid = attribute.atttypid \
             WHERE namespace.nspname = '_oo' AND relation.relname = $1 AND attribute.attname = $2 \
               AND attribute.attnum > 0 AND NOT attribute.attisdropped",
            &[&table, &column],
        )
        .await
        .map_err(db_error)?;
    let Some(row) = row else {
        return Err(PolicyError::PhysicalAbsent);
    };
    let name: String = row.get(0);
    let kind: String = row.get(1);
    let typmod: i32 = row.get(2);
    if kind == "b" && catalog::grantable_base_type(&name, typmod) {
        Ok(())
    } else {
        Err(PolicyError::TypeRefused {
            column: logical.to_string(),
        })
    }
}

async fn ensure_membership(
    tx: &mut Transaction<'_>,
    role: &str,
    login: &str,
) -> Result<(), PolicyError> {
    let rows = tx
        .query(
            "SELECT member_role.rolname::text, membership.inherit_option, \
                    membership.set_option, membership.admin_option \
             FROM pg_catalog.pg_auth_members AS membership \
             JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid \
             JOIN pg_catalog.pg_roles AS member_role ON member_role.oid = membership.member \
             WHERE granted.rolname = $1",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    if rows.len() != 1 {
        return Err(PolicyError::MembershipMismatch);
    }
    let name: String = rows[0].get(0);
    let inherit: bool = rows[0].get(1);
    let set_option: bool = rows[0].get(2);
    let admin_option: bool = rows[0].get(3);
    if name == login && !inherit && set_option && !admin_option {
        Ok(())
    } else {
        Err(PolicyError::MembershipMismatch)
    }
}

async fn ensure_members_preserved(
    tx: &mut Transaction<'_>,
    role: &str,
    login: &str,
) -> Result<(), PolicyError> {
    let rows = tx
        .query(
            "SELECT member_role.rolname::text, membership.inherit_option, \
                    membership.set_option, membership.admin_option \
             FROM pg_catalog.pg_auth_members AS membership \
             JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid \
             JOIN pg_catalog.pg_roles AS member_role ON member_role.oid = membership.member \
             WHERE granted.rolname = $1",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    if rows.is_empty() {
        return Err(PolicyError::MembershipMismatch);
    }
    let mut saw_login = false;
    for row in rows {
        let name: String = row.get(0);
        let inherit: bool = row.get(1);
        let set_option: bool = row.get(2);
        let admin_option: bool = row.get(3);
        if inherit || !set_option || admin_option {
            return Err(PolicyError::MembershipMismatch);
        }
        saw_login |= name == login;
    }
    if saw_login {
        Ok(())
    } else {
        Err(PolicyError::MembershipMismatch)
    }
}

async fn ensure_no_future_select(tx: &mut Transaction<'_>, role: &str) -> Result<(), PolicyError> {
    let row = tx
        .query_one(
            "SELECT count(*)::int8 \
             FROM pg_catalog.pg_default_acl AS d \
             LEFT JOIN pg_catalog.pg_namespace AS n ON n.oid = d.defaclnamespace \
             CROSS JOIN LATERAL pg_catalog.aclexplode(d.defaclacl) AS a \
             JOIN pg_catalog.pg_roles AS r ON r.oid = a.grantee \
             WHERE r.rolname = $1 AND a.privilege_type = 'SELECT' \
               AND d.defaclobjtype = 'r' \
               AND (n.nspname = '_oo' OR d.defaclnamespace = 0)",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    let count: i64 = row.get(0);
    if count == 0 {
        Ok(())
    } else {
        Err(PolicyError::GrantMismatch)
    }
}

async fn revoke_older_same_table_grants(
    tx: &mut Transaction<'_>,
    role: &str,
    before: &CompiledPolicySet,
) -> Result<(), PolicyError> {
    let quoted_role = quote_ident(role)?;
    for policy in &before.policies {
        let rows = tx
            .query(
                "SELECT DISTINCT m.physical_name::text \
                 FROM _oo.physical_mappings AS m \
                 JOIN information_schema.column_privileges AS p \
                   ON p.table_schema = '_oo' AND p.table_name = m.physical_name \
                  AND p.grantee = $1 AND p.privilege_type = 'SELECT' \
                 WHERE m.source = $2 AND m.config_digest = $3 AND m.level = 't' \
                   AND m.logical_namespace = $4 AND m.logical_table = $5 \
                   AND m.run_generation < $6",
                &[
                    &role,
                    &policy.project,
                    &policy.config_digest,
                    &policy.namespace,
                    &policy.table,
                    &policy.generation,
                ],
            )
            .await
            .map_err(db_error)?;
        for row in rows {
            let table: String = row.get(0);
            let table_name = require_physical(&table, "pt_")?;
            let columns = granted_columns(tx, role, &table).await?;
            if columns.is_empty() {
                continue;
            }
            let mut quoted = Vec::with_capacity(columns.len());
            for column in &columns {
                quoted.push(require_physical(column, "pc_")?);
            }
            let list = quoted.join(", ");
            tx.batch_execute(&format!(
                "REVOKE SELECT ({list}) ON TABLE \"_oo\".{table_name} FROM {quoted_role}"
            ))
            .await
            .map_err(db_error)?;
        }
    }
    Ok(())
}

async fn ensure_exact_grants(
    tx: &mut Transaction<'_>,
    role: &str,
    tables: &[ResolvedTable],
) -> Result<(), PolicyError> {
    let rows = tx
        .query(
            "SELECT table_schema::text, table_name::text, column_name::text, privilege_type::text \
             FROM information_schema.column_privileges WHERE grantee = $1",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    let mut actual = BTreeSet::new();
    for row in rows {
        let schema: String = row.get(0);
        let table: String = row.get(1);
        let column: String = row.get(2);
        let privilege: String = row.get(3);
        actual.insert((schema, table, column, privilege));
    }
    let mut expected = BTreeSet::new();
    for table in tables {
        for column in &table.physical_columns {
            expected.insert((
                "_oo".to_string(),
                table.physical_table.clone(),
                column.clone(),
                "SELECT".to_string(),
            ));
        }
        ensure_metadata_hidden(tx, role, &table.physical_table).await?;
    }
    if actual != expected {
        return Err(PolicyError::GrantMismatch);
    }
    let row = tx
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.table_privileges WHERE grantee = $1",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    let count: i64 = row.get(0);
    if count == 0 {
        Ok(())
    } else {
        Err(PolicyError::GrantMismatch)
    }
}

async fn ensure_metadata_hidden(
    tx: &mut Transaction<'_>,
    role: &str,
    table: &str,
) -> Result<(), PolicyError> {
    let row = tx
        .query_one(
            "SELECT pg_catalog.has_column_privilege(($1::text)::name, relation.oid, '_oo_pos'::text, 'SELECT') \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname = $2",
            &[&role, &table],
        )
        .await
        .map_err(db_error)?;
    let allowed: Option<bool> = row.get(0);
    if allowed == Some(true) {
        Err(PolicyError::MetadataColumn {
            column: "_oo_pos".to_string(),
        })
    } else {
        Ok(())
    }
}

async fn ensure_schema_usage(tx: &mut Transaction<'_>, role: &str) -> Result<(), PolicyError> {
    let row = tx
        .query_one(
            "SELECT pg_catalog.has_schema_privilege(($1::text)::name, '_oo'::name, 'USAGE'), \
                    pg_catalog.has_schema_privilege(($1::text)::name, '_oo'::name, 'CREATE')",
            &[&role],
        )
        .await
        .map_err(db_error)?;
    let usage: bool = row.get(0);
    let create: bool = row.get(1);
    if usage && !create {
        Ok(())
    } else {
        Err(PolicyError::GrantMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, pii: PiiClass) -> ColumnSpec {
        ColumnSpec {
            name: name.to_string(),
            status: ClassStatus::Confirmed,
            semantic_type: "text".to_string(),
            pii,
        }
    }

    fn policy(filter: Option<RowFilter>, columns: Vec<ColumnSpec>) -> ColumnAclPolicy {
        ColumnAclPolicy {
            project: "orders".to_string(),
            config_digest: "ab".repeat(32),
            generation: 7,
            role: "dispatcher".to_string(),
            login: "oo_a171_login".to_string(),
            table: LogicalTable {
                namespace: "logistics".to_string(),
                name: "shipment".to_string(),
                columns,
            },
            row_filter: filter,
        }
    }

    #[test]
    fn row_filter_is_refused_without_a_partial_allowlist() {
        let input = policy(
            Some(RowFilter::Claims {
                column: "region".to_string(),
                claim: "region".to_string(),
            }),
            vec![column("order_id", PiiClass::None)],
        );
        assert_eq!(
            compile_column_acl(&input).unwrap_err(),
            PolicyError::RowFilterRefused
        );
    }

    #[test]
    fn unknown_pii_and_unclassified_columns_are_refused() {
        let unknown = policy(None, vec![column("secret", PiiClass::Unknown)]);
        assert!(matches!(
            compile_column_acl(&unknown),
            Err(PolicyError::ClassificationRefused { .. })
        ));
        let mut draft = policy(None, vec![column("order_id", PiiClass::None)]);
        draft.table.columns[0].status = ClassStatus::Unclassified;
        assert!(matches!(
            compile_column_acl(&draft),
            Err(PolicyError::ClassificationRefused { .. })
        ));
        let mut hidden = policy(
            None,
            vec![column(
                "secret",
                PiiClass::Declared {
                    explicit_allow: false,
                },
            )],
        );
        hidden.table.columns[0].semantic_type.clear();
        assert!(matches!(
            compile_column_acl(&hidden),
            Err(PolicyError::ClassificationRefused { .. })
        ));
    }

    #[test]
    fn identifiers_reject_sql_and_the_owner_role() {
        let mut input = policy(None, vec![column("order_id", PiiClass::None)]);
        input.role = "dispatcher;drop".to_string();
        assert!(matches!(
            compile_column_acl(&input),
            Err(PolicyError::InvalidIdentifier { field: "role" })
        ));
        input.role = "dispatcher".to_string();
        input.login = "oo_target_owner".to_string();
        assert!(matches!(
            compile_column_acl(&input),
            Err(PolicyError::InvalidIdentifier { field: "login" })
        ));
        input.login = "oo_a171_login".to_string();
        input.table.columns[0].name = "OrderId".to_string();
        assert!(matches!(
            compile_column_acl(&input),
            Err(PolicyError::InvalidIdentifier { field: "column" })
        ));
    }

    #[test]
    fn native_identifiers_preserve_case_without_accepting_sql_syntax() {
        let mut input = policy(None, vec![column("whCode", PiiClass::None)]);
        assert_eq!(
            compile_column_acl_native(&input).unwrap().columns[0].name,
            "whCode"
        );
        assert!(matches!(
            compile_column_acl(&input),
            Err(PolicyError::InvalidIdentifier { field: "column" })
        ));
        for bad in ["wh\"Code", "wh;Code", "wh\nCode", "éCode"] {
            input.table.columns[0].name = bad.to_string();
            assert!(matches!(
                compile_column_acl_native(&input),
                Err(PolicyError::InvalidIdentifier { field: "column" })
            ));
        }
        input.table.columns[0].name = "x".repeat(64);
        assert!(compile_column_acl_native(&input).is_err());
    }

    #[test]
    fn an_empty_allowlist_is_refused() {
        let input = policy(None, Vec::new());
        assert_eq!(
            compile_column_acl(&input).unwrap_err(),
            PolicyError::EmptyAllowlist
        );
    }

    #[test]
    fn native_vector_advances_one_table_and_keeps_another_current() {
        let mut kept = policy(None, vec![column("order_id", PiiClass::None)]);
        kept.table.name = "alpha".to_string();
        kept.generation = 3;
        let mut moving = kept.clone();
        moving.table.name = "beta".to_string();
        moving.generation = 5;
        let mut moved = moving.clone();
        moved.generation = 13;
        let before = vec![kept.clone(), moving];
        let after = vec![kept.clone(), moved.clone()];
        assert!(compile_column_acl_native_vector_set(&after).is_ok());
        assert!(compile_column_acl_native_transition(&before, &after).is_ok());
        assert!(compile_column_acl_native_transition(&after, &after).is_err());
        assert!(compile_column_acl_native_transition(&before, &[kept.clone()]).is_err());
        let mut extra = kept.clone();
        extra.table.name = "gamma".to_string();
        extra.generation = 13;
        assert!(compile_column_acl_native_transition(&before, &[kept, moved.clone(), extra]).is_err());
        let mut back = moved.clone();
        back.generation = 5;
        assert!(compile_column_acl_native_transition(&[moved], &[back]).is_err());
    }

    #[test]
    fn native_transition_only_advances_generations_with_identical_authority() {
        let old = policy(None, vec![column("order_id", PiiClass::None)]);
        let mut next = old.clone(); next.generation += 1;
        assert!(compile_column_acl_native_transition(&[old.clone()], &[next.clone()]).is_ok());
        let mut third = next.clone(); third.generation += 1;
        assert!(compile_column_acl_native_transition(&[next.clone()], &[third]).is_ok());
        assert!(compile_column_acl_native_transition(&[old.clone()], &[old.clone()]).is_err());
        assert!(compile_column_acl_native_transition(&[next.clone()], &[old.clone()]).is_err());
        for field in 0..8 {
            let mut bad = next.clone();
            match field {
                0 => bad.project = "other".into(),
                1 => bad.config_digest = "b".repeat(64),
                2 => bad.role = "other".into(),
                3 => bad.login = "other".into(),
                4 => bad.table.namespace = "other".into(),
                5 => bad.table.columns[0].name = "other".into(),
                6 => bad.table.columns[0].semantic_type = "other".into(),
                _ => bad.table.columns[0].pii = PiiClass::Declared { explicit_allow:true },
            }
            assert!(compile_column_acl_native_transition(&[old.clone()], &[bad]).is_err());
        }
        assert!(compile_column_acl_native_transition(&[old], &[]).is_err());
    }

    #[test]
    fn a_confirmed_allowlist_compiles_to_one_nologin_role() {
        let compiled = compile_column_acl(&policy(
            None,
            vec![
                column("order_id", PiiClass::None),
                column(
                    "secret",
                    PiiClass::Declared {
                        explicit_allow: true,
                    },
                ),
            ],
        ))
        .expect("compile");
        assert_eq!(compiled.role, "oo_r_dispatcher");
        assert_eq!(compiled.generation, 7);
        assert_eq!(compiled.columns.len(), 2);
    }

    #[test]
    fn a_set_refuses_empty_mixed_duplicate_bounds_and_metadata() {
        let one = policy(None, vec![column("order_id", PiiClass::None)]);
        assert_eq!(
            compile_column_acl_set(&[]).unwrap_err(),
            PolicyError::EmptyPolicySet
        );
        let mut other_role = one.clone();
        other_role.role = "other".to_string();
        other_role.table.name = "invoice".to_string();
        assert_eq!(
            compile_column_acl_set(&[one.clone(), other_role]).unwrap_err(),
            PolicyError::MixedIdentity
        );
        let mut other_generation = one.clone();
        other_generation.table.name = "invoice".to_string();
        other_generation.generation = 8;
        assert_eq!(
            compile_column_acl_set(&[one.clone(), other_generation]).unwrap_err(),
            PolicyError::MixedIdentity
        );
        assert_eq!(
            compile_column_acl_set(&[one.clone(), one.clone()]).unwrap_err(),
            PolicyError::InvalidIdentifier { field: "table" }
        );
        let mut wide = one.clone();
        wide.table.columns = (0..513)
            .map(|index| column(&format!("c{index}"), PiiClass::None))
            .collect();
        assert_eq!(
            compile_column_acl_set(&[wide]).unwrap_err(),
            PolicyError::LimitExceeded
        );
        let mut many = Vec::new();
        for index in 0..33 {
            let mut item = one.clone();
            item.table.name = format!("t{index}");
            many.push(item);
        }
        assert_eq!(
            compile_column_acl_set(&many).unwrap_err(),
            PolicyError::LimitExceeded
        );
        let mut hidden = one.clone();
        hidden.table.columns = vec![column("_oo_pos", PiiClass::None)];
        assert!(matches!(
            compile_column_acl_set(std::slice::from_ref(&hidden)),
            Err(PolicyError::MetadataColumn { .. })
        ));
        assert!(compile_column_acl(&hidden).is_ok());
        let mut filtered = one.clone();
        filtered.table.name = "invoice".to_string();
        filtered.row_filter = Some(RowFilter::Claims {
            column: "region".to_string(),
            claim: "region".to_string(),
        });
        assert_eq!(
            compile_column_acl_set(&[one.clone(), filtered]).unwrap_err(),
            PolicyError::RowFilterRefused
        );
        let mut second = one.clone();
        second.table.name = "invoice".to_string();
        let compiled = compile_column_acl_set(&[one, second]).expect("set");
        assert_eq!(compiled.len(), 2);
        assert!(!compiled.is_empty());
        assert_eq!(compiled.role(), "oo_r_dispatcher");
    }
}
