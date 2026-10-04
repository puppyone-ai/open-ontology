//! Access report and source catalog. These types are the frozen output.

use serde::{Deserialize, Serialize};

use crate::error::SourceError;
use crate::ident::hex_bytes;

/// SHA-256 of `{"tables":[]}`.
pub const EMPTY_SCHEMA_HASH: &str =
    "cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc";

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex_bytes(&Sha256::digest(bytes))
}

/// Text the server returned that this process must not execute or log.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UntrustedEvidence {
    /// UTF-8 byte length of the server text.
    pub byte_len: usize,
    /// SHA-256 of that UTF-8 text.
    pub sha256: String,
}

impl UntrustedEvidence {
    pub(crate) fn from_text(text: &str) -> Self {
        Self {
            byte_len: text.len(),
            sha256: sha256_hex(text.as_bytes()),
        }
    }
}

impl std::fmt::Debug for UntrustedEvidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UntrustedEvidence")
            .field("byte_len", &self.byte_len)
            .field("sha256", &self.sha256)
            .finish()
    }
}

/// Why this slice did not convert a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeUnsupportedReason {
    /// Builtin or composite type. The original oid is kept.
    KnownBuiltinNotConverted,
    /// Domain. `base_type_oid` is kept beside the domain oid.
    DomainNotConverted,
    /// Enum. The enum oid is kept.
    EnumNotConverted,
    /// `numeric` with precision above 38, or unconstrained `numeric`.
    UnboundedOrWideNumeric,
    /// Type oid this slice will not classify.
    UnknownTypeOid,
}

/// Explicit non-conversion of one column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeSupport {
    /// Always false in this slice.
    pub converted: bool,
    /// Why conversion was not performed.
    pub reason: TypeUnsupportedReason,
}

impl TypeSupport {
    pub(crate) fn unsupported(reason: TypeUnsupportedReason) -> Self {
        Self {
            converted: false,
            reason,
        }
    }
}

pub(crate) fn classify_type(typtype: &str, type_name: &str, typmod: i32) -> TypeSupport {
    if typtype == "d" {
        return TypeSupport::unsupported(TypeUnsupportedReason::DomainNotConverted);
    }
    if typtype == "e" {
        return TypeSupport::unsupported(TypeUnsupportedReason::EnumNotConverted);
    }
    if type_name == "numeric" {
        match numeric_parts(typmod) {
            None => {
                return TypeSupport::unsupported(TypeUnsupportedReason::UnboundedOrWideNumeric);
            }
            Some((precision, _)) if precision > 38 => {
                return TypeSupport::unsupported(TypeUnsupportedReason::UnboundedOrWideNumeric);
            }
            Some(_) => {}
        }
    }
    if matches!(typtype, "b" | "p" | "r" | "m" | "c") {
        return TypeSupport::unsupported(TypeUnsupportedReason::KnownBuiltinNotConverted);
    }
    TypeSupport::unsupported(TypeUnsupportedReason::UnknownTypeOid)
}

/// `numeric` typmod. `None` means unconstrained (`typmod = -1`).
pub fn numeric_parts(typmod: i32) -> Option<(i32, i32)> {
    if typmod < 0 {
        return None;
    }
    let adjusted = typmod - 4;
    let precision = (adjusted >> 16) & 0xFFFF;
    let scale = adjusted & 0xFFFF;
    Some((precision, scale))
}

/// One column in a key, in key order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyColumn {
    /// `pg_attribute.attnum`.
    pub attnum: i32,
    /// Original column name.
    pub name: String,
}

/// A non-primary unique index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniqueKey {
    /// Index name.
    pub name: String,
    /// Key columns in index order.
    pub columns: Vec<KeyColumn>,
    /// PostgreSQL 15+ `indnullsnotdistinct`.
    pub nulls_not_distinct: bool,
}

/// One foreign-key column pair.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignKeyColumn {
    /// Local attnum.
    pub attnum: i32,
    /// Local column name.
    pub name: String,
    /// Referenced schema name.
    pub ref_schema: String,
    /// Referenced table name.
    pub ref_table: String,
    /// Referenced attnum.
    pub ref_attnum: i32,
    /// Referenced column name.
    pub ref_name: String,
}

/// A foreign key leaving a selected table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignKey {
    /// Constraint name.
    pub name: String,
    /// Column pairs in constraint order.
    pub columns: Vec<ForeignKeyColumn>,
}

/// Row-security flags for one selected table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RlsTableFact {
    /// Schema name.
    pub schema: String,
    /// Table name.
    pub table: String,
    /// `relrowsecurity`. Unknown when the probe returned NULL.
    pub enabled: Option<bool>,
    /// `relforcerowsecurity`.
    pub forced: Option<bool>,
    /// Policies visible on the table.
    pub policy_count: Option<u32>,
}

/// A row-security policy. Expressions are hashes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRecord {
    /// Policy name.
    pub name: String,
    /// `pg_policy.polcmd` (`*` for all commands).
    pub command: String,
    /// Permissive policies combine with OR.
    pub permissive: bool,
    /// Hash of the USING expression.
    pub using_expr: Option<UntrustedEvidence>,
    /// Hash of the WITH CHECK expression.
    pub check_expr: Option<UntrustedEvidence>,
}

/// One ACL entry visible on the table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegeGrant {
    /// Role name, or `public`.
    pub grantee: String,
    /// Privilege type such as `SELECT` or `INSERT`.
    pub privilege: String,
    /// Whether the grant carries grant option.
    pub grantable: bool,
}

/// Privileges of the connected principal on one column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnPrivileges {
    /// `SELECT`, or unknown when the probe returned NULL.
    pub select: Option<bool>,
    /// `INSERT`.
    pub insert: Option<bool>,
    /// `UPDATE`.
    pub update: Option<bool>,
}

/// One catalog column. Type conversion is explicitly unsupported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceColumn {
    /// `pg_attribute.attnum`.
    pub attnum: i32,
    /// Original column name.
    pub name: String,
    /// Stable identifier.
    pub name_ident: String,
    /// `pg_attribute.atttypid`. Domains and enums keep their own oid.
    pub type_oid: i64,
    /// Schema of the type.
    pub type_schema: String,
    /// Type name.
    pub type_name: String,
    /// `pg_type.typtype` (`b`, `d`, `e`, `c`, ...).
    pub typtype: String,
    /// `pg_attribute.atttypmod`.
    pub typmod: i32,
    /// Base type oid for a domain. Absent for other types.
    pub base_type_oid: Option<i64>,
    /// Numeric precision when `typmod` encodes one.
    pub numeric_precision: Option<i32>,
    /// Numeric scale when `typmod` encodes one.
    pub numeric_scale: Option<i32>,
    /// `attnotnull`.
    pub not_null: bool,
    /// Collation oid. Zero means no collation.
    pub collation_oid: i64,
    /// Collation name when one is set.
    pub collation_name: Option<String>,
    /// `attidentity`: empty, `a`, or `d`.
    pub identity: String,
    /// `attgenerated`: empty or `s`.
    pub generated: String,
    /// Hash of the default expression.
    pub default_expr: Option<UntrustedEvidence>,
    /// Hash of the column comment.
    pub comment: Option<UntrustedEvidence>,
    /// Enum labels in sort order. Empty unless `typtype` is `e`.
    pub enum_labels: Vec<String>,
    /// Hashes of domain check definitions.
    pub domain_checks: Vec<UntrustedEvidence>,
    /// Conversion status. `converted` is false.
    pub type_support: TypeSupport,
    /// Privileges of the connected principal.
    pub current_privileges: ColumnPrivileges,
}

/// One selected table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTable {
    /// Original schema name.
    pub schema: String,
    /// Stable schema identifier.
    pub schema_ident: String,
    /// Original table name.
    pub name: String,
    /// Stable table identifier.
    pub name_ident: String,
    /// `pg_class.relkind`.
    pub relkind: String,
    /// `pg_class.relreplident` as text.
    pub replica_identity: String,
    /// `relrowsecurity`.
    pub rls_enabled: bool,
    /// `relforcerowsecurity`.
    pub rls_forced: bool,
    /// Owner role name.
    pub owner: String,
    /// Hash of the table comment.
    pub comment: Option<UntrustedEvidence>,
    /// Columns in attnum order.
    pub columns: Vec<SourceColumn>,
    /// Primary key columns in key order. Empty when the table has no primary key.
    pub primary_key: Vec<KeyColumn>,
    /// `indnullsnotdistinct` of the primary key. Absent when there is no primary key.
    pub primary_key_nulls_not_distinct: Option<bool>,
    /// Unique indexes that are not the primary key.
    pub unique_keys: Vec<UniqueKey>,
    /// Outgoing foreign keys.
    pub foreign_keys: Vec<ForeignKey>,
    /// Row-security policies.
    pub policies: Vec<PolicyRecord>,
    /// Table ACL, including the default owner ACL when `relacl` is null.
    pub privileges: Vec<PrivilegeGrant>,
}

/// Authoritative catalog snapshot for the selected tables.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCatalog {
    /// `oo.source.pgsql/v1`.
    pub protocol: String,
    /// `oo_core::CONTRACT_CONFIG`.
    pub core_contract: String,
    /// Trusted source id.
    pub source_id: String,
    /// Database name reported by the server.
    pub database: String,
    /// `transaction_timestamp()` in UTC, excluded from the schema hash.
    pub captured_at: String,
    /// SHA-256 of the canonical structural preimage.
    pub schema_hash: String,
    /// `pg-catalog-hash/v1`.
    pub hash_algorithm: String,
    /// `pgident/v1`.
    pub ident_algorithm: String,
    /// Selected tables.
    pub tables: Vec<SourceTable>,
    /// Always `unsupported` in this slice.
    pub type_conversion: String,
}

/// Canonical structural hash. Comments, privilege grants, and the capture
/// timestamp are not part of the preimage. Policy and default bodies are
/// represented by their hashes.
pub fn schema_hash(tables: &[SourceTable]) -> String {
    let mut ordered: Vec<&SourceTable> = tables.iter().collect();
    ordered.sort_by(|left, right| (&left.schema, &left.name).cmp(&(&right.schema, &right.name)));
    let mut buf = String::from("{\"tables\":[");
    for (index, table) in ordered.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        write_table(&mut buf, table);
    }
    buf.push_str("]}");
    sha256_hex(buf.as_bytes())
}

fn write_table(buf: &mut String, table: &SourceTable) {
    buf.push('{');
    write_str_field(buf, "schema", &table.schema);
    buf.push(',');
    write_str_field(buf, "name", &table.name);
    buf.push(',');
    write_str_field(buf, "relkind", &table.relkind);
    buf.push(',');
    write_str_field(buf, "replica_identity", &table.replica_identity);
    buf.push(',');
    write_bool_field(buf, "rls", table.rls_enabled);
    buf.push(',');
    write_bool_field(buf, "force_rls", table.rls_forced);
    buf.push_str(",\"columns\":[");
    for (index, column) in table.columns.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        write_column(buf, column);
    }
    buf.push_str("],\"primary_key\":[");
    write_key_columns(buf, &table.primary_key);
    buf.push_str("],\"primary_key_nulls_not_distinct\":");
    write_opt_bool(buf, table.primary_key_nulls_not_distinct);
    buf.push_str(",\"unique_keys\":[");
    let mut uniques = table.unique_keys.clone();
    uniques.sort_by(|left, right| left.name.cmp(&right.name));
    for (index, key) in uniques.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_str_field(buf, "name", &key.name);
        buf.push_str(",\"columns\":[");
        write_key_columns(buf, &key.columns);
        buf.push_str("],");
        write_bool_field(buf, "nulls_not_distinct", key.nulls_not_distinct);
        buf.push('}');
    }
    buf.push_str("],\"foreign_keys\":[");
    let mut foreign = table.foreign_keys.clone();
    foreign.sort_by(|left, right| left.name.cmp(&right.name));
    for (index, key) in foreign.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_str_field(buf, "name", &key.name);
        buf.push_str(",\"columns\":[");
        for (col_index, column) in key.columns.iter().enumerate() {
            if col_index > 0 {
                buf.push(',');
            }
            buf.push('{');
            write_i32_field(buf, "attnum", column.attnum);
            buf.push(',');
            write_str_field(buf, "name", &column.name);
            buf.push(',');
            write_str_field(buf, "ref_schema", &column.ref_schema);
            buf.push(',');
            write_str_field(buf, "ref_table", &column.ref_table);
            buf.push(',');
            write_i32_field(buf, "ref_attnum", column.ref_attnum);
            buf.push(',');
            write_str_field(buf, "ref_name", &column.ref_name);
            buf.push('}');
        }
        buf.push_str("]}");
    }
    buf.push_str("],\"policies\":[");
    let mut policies = table.policies.clone();
    policies.sort_by(|left, right| left.name.cmp(&right.name));
    for (index, policy) in policies.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_str_field(buf, "name", &policy.name);
        buf.push(',');
        write_str_field(buf, "command", &policy.command);
        buf.push(',');
        write_bool_field(buf, "permissive", policy.permissive);
        buf.push(',');
        write_evidence_field(buf, "using_sha256", policy.using_expr.as_ref());
        buf.push(',');
        write_evidence_field(buf, "check_sha256", policy.check_expr.as_ref());
        buf.push('}');
    }
    buf.push_str("]}");
}

fn write_column(buf: &mut String, column: &SourceColumn) {
    buf.push('{');
    write_i32_field(buf, "attnum", column.attnum);
    buf.push(',');
    write_str_field(buf, "name", &column.name);
    buf.push(',');
    write_i64_field(buf, "type_oid", column.type_oid);
    buf.push(',');
    write_i32_field(buf, "typmod", column.typmod);
    buf.push(',');
    write_bool_field(buf, "not_null", column.not_null);
    buf.push(',');
    write_i64_field(buf, "collation_oid", column.collation_oid);
    buf.push(',');
    write_str_field(buf, "typtype", &column.typtype);
    buf.push(',');
    write_opt_i64_field(buf, "base_type_oid", column.base_type_oid);
    buf.push(',');
    write_str_field(buf, "identity", &column.identity);
    buf.push(',');
    write_str_field(buf, "generated", &column.generated);
    buf.push(',');
    write_evidence_field(buf, "default_sha256", column.default_expr.as_ref());
    buf.push_str(",\"enum_labels\":[");
    for (index, label) in column.enum_labels.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        push_json_string(buf, label);
    }
    buf.push_str("],\"domain_checks\":[");
    for (index, check) in column.domain_checks.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        push_json_string(buf, &check.sha256);
    }
    buf.push_str("]}");
}

fn write_key_columns(buf: &mut String, columns: &[KeyColumn]) {
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_i32_field(buf, "attnum", column.attnum);
        buf.push(',');
        write_str_field(buf, "name", &column.name);
        buf.push('}');
    }
}

fn write_str_field(buf: &mut String, name: &str, value: &str) {
    push_json_string(buf, name);
    buf.push(':');
    push_json_string(buf, value);
}

fn write_bool_field(buf: &mut String, name: &str, value: bool) {
    push_json_string(buf, name);
    buf.push(':');
    buf.push_str(if value { "true" } else { "false" });
}

fn write_i32_field(buf: &mut String, name: &str, value: i32) {
    push_json_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_i64_field(buf: &mut String, name: &str, value: i64) {
    push_json_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_opt_i64_field(buf: &mut String, name: &str, value: Option<i64>) {
    push_json_string(buf, name);
    buf.push(':');
    match value {
        Some(inner) => buf.push_str(&inner.to_string()),
        None => buf.push_str("null"),
    }
}

fn write_opt_bool(buf: &mut String, value: Option<bool>) {
    match value {
        Some(true) => buf.push_str("true"),
        Some(false) => buf.push_str("false"),
        None => buf.push_str("null"),
    }
}

fn write_evidence_field(buf: &mut String, name: &str, value: Option<&UntrustedEvidence>) {
    push_json_string(buf, name);
    buf.push(':');
    match value {
        Some(evidence) => push_json_string(buf, &evidence.sha256),
        None => buf.push_str("null"),
    }
}

fn push_json_string(buf: &mut String, value: &str) {
    buf.push('"');
    for ch in value.chars() {
        match ch {
            '"' => buf.push_str("\\\""),
            '\\' => buf.push_str("\\\\"),
            '\u{0008}' => buf.push_str("\\b"),
            '\u{000c}' => buf.push_str("\\f"),
            '\n' => buf.push_str("\\n"),
            '\r' => buf.push_str("\\r"),
            '\t' => buf.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                buf.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => buf.push(ch),
        }
    }
    buf.push('"');
}

/// Known, known-absent, unknown, or not collected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactStatus {
    /// The server answered.
    Known,
    /// The server answered and the object is absent.
    Absent,
    /// The server could not show the fact. This is not absence.
    Unknown,
    /// This slice did not ask, because the version is outside the tested set.
    NotCollected,
}

/// One inspected fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact<T> {
    /// How the fact was obtained.
    pub status: FactStatus,
    /// Present when `status` is `known`.
    pub value: Option<T>,
    /// Static reason when the fact is unknown or not collected.
    pub reason: Option<String>,
}

impl<T> Fact<T> {
    pub(crate) fn known(value: T) -> Self {
        Self {
            status: FactStatus::Known,
            value: Some(value),
            reason: None,
        }
    }

    pub(crate) fn absent() -> Self {
        Self {
            status: FactStatus::Absent,
            value: None,
            reason: None,
        }
    }

    pub(crate) fn unknown(reason: &'static str) -> Self {
        Self {
            status: FactStatus::Unknown,
            value: None,
            reason: Some(reason.to_string()),
        }
    }

    pub(crate) fn not_collected(reason: &'static str) -> Self {
        Self {
            status: FactStatus::NotCollected,
            value: None,
            reason: Some(reason.to_string()),
        }
    }
}

/// Why registration is blocked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockReason {
    /// Stable code.
    pub code: String,
    /// Static detail.
    pub fact: String,
}

impl BlockReason {
    pub(crate) fn new(code: &'static str, fact: &'static str) -> Self {
        Self {
            code: code.to_string(),
            fact: fact.to_string(),
        }
    }
}

/// Connected principal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    /// `current_user`.
    pub current_user: String,
    /// `session_user`.
    pub session_user: String,
    /// `current_database`.
    pub database: String,
}

/// Role attributes that decide privilege and escalation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleAttributes {
    /// Role name.
    pub name: String,
    /// `rolsuper`.
    pub superuser: bool,
    /// `rolinherit`.
    pub inherit: bool,
    /// `rolcreaterole`.
    pub create_role: bool,
    /// `rolcreatedb`.
    pub create_db: bool,
    /// `rolcanlogin`.
    pub can_login: bool,
    /// `rolreplication`. This alone does not block registration.
    pub replication: bool,
    /// `rolbypassrls`.
    pub bypass_rls: bool,
}

/// Direct membership edge from the connected role.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleMembership {
    /// Granted role.
    pub role: String,
    /// `pg_auth_members.inherit_option`.
    pub inherit_option: bool,
    /// `pg_auth_members.set_option`.
    pub set_option: bool,
    /// `pg_auth_members.admin_option`. Treated as SET ROLE reachability.
    pub admin_option: bool,
}

/// How a write privilege was reached without issuing a write.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteFinding {
    /// Schema name. Empty for a role-level finding.
    pub schema: String,
    /// Table name. Empty for a role-level finding.
    pub table: String,
    /// Role that holds the privilege.
    pub role: String,
    /// Privilege or attribute name.
    pub privilege: String,
    /// `direct`, `inherited`, `set_role`, `owner`, `schema_create`, `column`, or `database`.
    pub path: String,
}

/// Result of the write probe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteDetermination {
    /// `read_only`, `write_capable`, or `unknown`.
    pub state: String,
    /// Findings. Empty when `state` is `read_only`.
    pub findings: Vec<WriteFinding>,
}

/// TLS mode actually requested by this driver.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlsReport {
    /// Always `verify-full`.
    pub mode: String,
    /// `caller-ca-only`. The caller PEM is the only trust anchor.
    pub trust: String,
    /// Hostname verification is on.
    pub hostname_verification: bool,
    /// `pg_stat_ssl.version` when `server_observation` is `observed`, for
    /// example `TLSv1.3`. `unknown` when that view returns no row. `unknown`
    /// is not a protocol version.
    pub negotiated_version: String,
    /// `observed` when `pg_stat_ssl` returned `ssl = true` and a `TLSv1.`
    /// version for this backend. `unknown` when the view returned no row.
    /// Absence is not plaintext and is not proof that TLS was negotiated.
    /// `mode` stays `verify-full` either way.
    pub server_observation: String,
}

/// One capability this connector reports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityStatus {
    /// Capability name.
    pub name: String,
    /// True when this build advertises the capability.
    pub supported: bool,
    /// Why it is supported or unsupported.
    pub reason: String,
}

/// Static capability list. CDC prerequisites are separate facts.
pub fn connector_capabilities() -> Vec<CapabilityStatus> {
    const READER: &str =
        "PostgreSQL full-mode snapshot plus bounded pull and RunComplete admission";
    [
        ("scan", true, READER),
        ("bounded_read", true, READER),
        ("cdc", false, "not_in_this_slice"),
        ("type_conversion", false, "not_in_this_slice"),
        ("registry_api", false, "not_in_this_slice"),
        ("source_ddl", false, "forbidden"),
        ("source_write", false, "forbidden"),
    ]
    .into_iter()
    .map(|(name, supported, reason)| CapabilityStatus {
        name: name.to_string(),
        supported,
        reason: reason.to_string(),
    })
    .collect()
}

/// CDC inputs. `implemented` stays false even when the inputs are present.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CdcPrerequisite {
    /// `wal_level`.
    pub wal_level: Fact<String>,
    /// Whether an assumable role has `rolreplication`.
    pub replication_attribute: Fact<bool>,
    /// Whether a publication covers every selected table.
    pub publication_for_selected: Fact<bool>,
    /// Always false. This slice does not stream changes.
    pub implemented: bool,
}

/// A security-definer function the principal can execute. The body is not included.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinerFunction {
    /// Schema name.
    pub schema: String,
    /// Function name.
    pub name: String,
    /// Owner role.
    pub owner: String,
}

/// One DBA checklist entry. The statement is text for a person, not a command
/// this process sends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbaItem {
    /// Stable item id.
    pub id: String,
    /// `info`, `warn`, or `required`.
    pub level: String,
    /// What can go wrong.
    pub risk: String,
    /// How a DBA undoes the change.
    pub rollback: String,
    /// Example statement. The password slot is `<由 DBA 设置>`.
    pub statement: String,
}

/// Server version admission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionFact {
    /// `server_version_num`.
    pub server_version_num: i64,
    /// `server_version`, capped by the driver.
    pub server_version: String,
    /// `server_version_num / 10000`.
    pub major: i64,
    /// `server_version_num % 10000`.
    pub minor: i64,
    /// Declared floor. Compatibility below the tested set is not claimed.
    pub declared_minimum_major: i64,
    /// Versions this build has been tested against.
    pub tested_versions: Vec<String>,
    /// `tested`, `untested`, `below_minimum`, or `unknown`.
    pub admission: String,
}

/// `inspect_access` result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessReport {
    /// `oo.source.pgsql/v1`.
    pub protocol: String,
    /// `oo_core::CONTRACT_CONFIG`.
    pub core_contract: String,
    /// Trusted source id.
    pub source_id: String,
    /// Secret reference. Not the secret value.
    pub secret_ref: String,
    /// TLS mode this driver requested.
    pub tls: TlsReport,
    /// Version admission.
    pub version: VersionFact,
    /// False when any block reason is present.
    pub registration_allowed: bool,
    /// Reasons registration is blocked.
    pub block_reasons: Vec<BlockReason>,
    /// Principal identity.
    pub principal: Fact<Principal>,
    /// `pg_is_in_recovery()`.
    pub in_recovery: Fact<bool>,
    /// Current role attributes.
    pub role: Fact<RoleAttributes>,
    /// Direct membership edges.
    pub memberships: Vec<RoleMembership>,
    /// Write probe.
    pub write: WriteDetermination,
    /// Security-definer functions the principal can execute.
    pub security_definers: Vec<DefinerFunction>,
    /// Row-security flags for selected tables.
    pub rls: Vec<RlsTableFact>,
    /// Publication names when the catalog is readable.
    pub publications: Fact<Vec<String>>,
    /// Replication slots. Limited visibility is `unknown`, not absent.
    pub replication_slots: Fact<Vec<String>>,
    /// CDC inputs. Implementation remains false.
    pub cdc: CdcPrerequisite,
    /// Capabilities. `scan` and `bounded_read` are supported. The other entries are not.
    pub capabilities: Vec<CapabilityStatus>,
    /// Objects a DBA would create. This process does not create them.
    pub dba_checklist: Vec<DbaItem>,
}

pub(crate) fn tls_report(negotiated_version: &str) -> TlsReport {
    let observed = negotiated_version.starts_with("TLSv1.");
    TlsReport {
        mode: "verify-full".to_string(),
        trust: "caller-ca-only".to_string(),
        hostname_verification: true,
        negotiated_version: negotiated_version.to_string(),
        server_observation: if observed {
            "observed".to_string()
        } else {
            "unknown".to_string()
        },
    }
}

pub(crate) fn push_block(reasons: &mut Vec<BlockReason>, code: &'static str, fact: &'static str) {
    let reason = BlockReason::new(code, fact);
    if !reasons.iter().any(|item| item == &reason) {
        reasons.push(reason);
    }
}

pub(crate) fn charge(used: &mut u64, max: u64, extra: u64) -> Result<(), SourceError> {
    *used = used.saturating_add(extra);
    if *used > max {
        Err(SourceError::budget("max_bytes"))
    } else {
        Ok(())
    }
}

pub(crate) fn admit_version(major: i64, minor: i64, raw: &str) -> &'static str {
    if major <= 0 {
        "unknown"
    } else if major < crate::DECLARED_MIN_MAJOR {
        "below_minimum"
    } else if major == 17 && minor == 10 && raw.starts_with("17.10") {
        "tested"
    } else if major >= crate::DECLARED_MIN_MAJOR {
        "untested"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str) -> SourceColumn {
        SourceColumn {
            attnum: 1,
            name: name.to_string(),
            name_ident: name.to_string(),
            type_oid: 23,
            type_schema: "pg_catalog".to_string(),
            type_name: "int4".to_string(),
            typtype: "b".to_string(),
            typmod: -1,
            base_type_oid: None,
            numeric_precision: None,
            numeric_scale: None,
            not_null: true,
            collation_oid: 0,
            collation_name: None,
            identity: String::new(),
            generated: String::new(),
            default_expr: None,
            comment: None,
            enum_labels: Vec::new(),
            domain_checks: Vec::new(),
            type_support: classify_type("b", "int4", -1),
            current_privileges: ColumnPrivileges {
                select: Some(true),
                insert: Some(false),
                update: Some(false),
            },
        }
    }

    fn table(columns: Vec<SourceColumn>, comment: Option<&str>) -> SourceTable {
        SourceTable {
            schema: "app".to_string(),
            schema_ident: "app".to_string(),
            name: "item".to_string(),
            name_ident: "item".to_string(),
            relkind: "r".to_string(),
            replica_identity: "d".to_string(),
            rls_enabled: false,
            rls_forced: false,
            owner: "owner".to_string(),
            comment: comment.map(UntrustedEvidence::from_text),
            columns,
            primary_key: vec![KeyColumn {
                attnum: 1,
                name: "id".to_string(),
            }],
            primary_key_nulls_not_distinct: Some(false),
            unique_keys: Vec::new(),
            foreign_keys: Vec::new(),
            policies: Vec::new(),
            privileges: Vec::new(),
        }
    }

    #[test]
    fn empty_hash_is_frozen_and_ignores_comments_and_privileges() {
        assert_eq!(schema_hash(&[]), EMPTY_SCHEMA_HASH);
        let mut base = table(vec![column("id")], None);
        let commented = table(vec![column("id")], Some("CANARY-COMMENT-SECRET-a060"));
        assert_eq!(
            schema_hash(std::slice::from_ref(&base)),
            schema_hash(std::slice::from_ref(&commented))
        );
        base.privileges.push(PrivilegeGrant {
            grantee: "reader".to_string(),
            privilege: "INSERT".to_string(),
            grantable: false,
        });
        assert_eq!(
            schema_hash(&[table(vec![column("id")], None)]),
            schema_hash(&[base])
        );
    }

    #[test]
    fn added_column_and_policy_change_the_hash() {
        let base = table(vec![column("id")], None);
        let mut extra = column("extra");
        extra.attnum = 2;
        extra.name_ident = "extra".to_string();
        let wider = table(vec![column("id"), extra], None);
        assert_ne!(
            schema_hash(std::slice::from_ref(&base)),
            schema_hash(std::slice::from_ref(&wider))
        );
        let mut with_policy = base.clone();
        with_policy.policies.push(PolicyRecord {
            name: "p".to_string(),
            command: "*".to_string(),
            permissive: true,
            using_expr: Some(UntrustedEvidence::from_text("true")),
            check_expr: None,
        });
        assert_ne!(schema_hash(&[base]), schema_hash(&[with_policy]));
    }

    #[test]
    fn wide_numeric_domain_and_enum_are_explicit() {
        assert!(matches!(
            classify_type("b", "numeric", -1).reason,
            TypeUnsupportedReason::UnboundedOrWideNumeric
        ));
        let wide = (((80_i32) << 16) | 10) + 4;
        assert!(matches!(
            classify_type("b", "numeric", wide).reason,
            TypeUnsupportedReason::UnboundedOrWideNumeric
        ));
        assert_eq!(numeric_parts(wide), Some((80, 10)));
        let narrow = (((10_i32) << 16) | 2) + 4;
        assert!(matches!(
            classify_type("b", "numeric", narrow).reason,
            TypeUnsupportedReason::KnownBuiltinNotConverted
        ));
        assert!(matches!(
            classify_type("d", "sku_code", -1).reason,
            TypeUnsupportedReason::DomainNotConverted
        ));
        assert!(matches!(
            classify_type("e", "order_status", -1).reason,
            TypeUnsupportedReason::EnumNotConverted
        ));
        let caps = connector_capabilities();
        let supported: Vec<&str> = caps
            .iter()
            .filter(|item| item.supported)
            .map(|item| item.name.as_str())
            .collect();
        let unsupported: Vec<&str> = caps
            .iter()
            .filter(|item| !item.supported)
            .map(|item| item.name.as_str())
            .collect();
        assert_eq!(supported, ["scan", "bounded_read"]);
        assert_eq!(
            unsupported,
            [
                "cdc",
                "type_conversion",
                "registry_api",
                "source_ddl",
                "source_write"
            ]
        );
        assert!(caps.iter().filter(|item| item.supported).all(|item| {
            item.reason
                == "PostgreSQL full-mode snapshot plus bounded pull and RunComplete admission"
        }));
        assert_eq!(admit_version(17, 10, "17.10 (Debian)"), "tested");
        assert_eq!(admit_version(13, 0, "13.0"), "untested");
        assert_eq!(admit_version(12, 22, "12.22"), "below_minimum");
    }

    #[test]
    fn evidence_debug_hides_the_body() {
        let evidence = UntrustedEvidence::from_text("CANARY-COMMENT-SECRET-a060");
        let rendered = format!("{evidence:?}");
        assert!(!rendered.contains("CANARY"));
        assert_eq!(evidence.byte_len, "CANARY-COMMENT-SECRET-a060".len());
    }

    #[test]
    fn empty_catalog_fixture_matches_the_frozen_hash() {
        let text = include_str!("../../../contracts/source/pgsql/empty-catalog.json");
        let catalog: SourceCatalog = serde_json::from_str(text).expect("empty catalog fixture");
        assert_eq!(catalog.protocol, "oo.source.pgsql/v1");
        assert_eq!(catalog.schema_hash, EMPTY_SCHEMA_HASH);
        assert!(catalog.tables.is_empty());
        assert_eq!(catalog.type_conversion, "unsupported");
        let encoded = serde_json::to_string(&catalog).expect("encode");
        let again: SourceCatalog = serde_json::from_str(&encoded).expect("round trip");
        assert_eq!(again, catalog);
    }
}
