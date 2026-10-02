//! PostgreSQL catalog evidence, without a driver.
//!
//! The field set and `pg-catalog-hash/v1` preimage follow the frozen
//! `oo.source.pgsql/v1` contract. This module does not connect, grant, or
//! convert values. `type_support.converted` stays false.

use serde::Deserialize;
use serde_json::Value;

use crate::canonical::{push_string, sha256_hex};
use crate::error::DraftError;
use crate::ident::{assign_idents, stable_ident};
use crate::limits::{
    MAX_COLUMNS, MAX_DOMAIN_CHECKS, MAX_ENUM_LABELS, MAX_KEY_COLUMNS, MAX_KEYS, MAX_NAME_BYTES,
    MAX_POLICIES, MAX_PRIVILEGES, MAX_TABLES, MAX_UNTRUSTED_BYTES,
};

pub const PROTOCOL: &str = "oo.source.pgsql/v1";
pub const HASH_ALGORITHM: &str = "pg-catalog-hash/v1";
pub const IDENT_ALGORITHM: &str = "pgident/v1";
pub const CORE_CONTRACT: &str = "oo.config/v1";
pub const EMPTY_SCHEMA_HASH: &str =
    "cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceHash {
    pub byte_len: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyColumn {
    pub attnum: i32,
    pub name: String,
    pub ident: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueKey {
    pub name: String,
    pub ident: String,
    pub columns: Vec<KeyColumn>,
    pub nulls_not_distinct: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignColumn {
    pub attnum: i32,
    pub name: String,
    pub ident: String,
    pub ref_schema: String,
    pub ref_schema_ident: String,
    pub ref_table: String,
    pub ref_table_ident: String,
    pub ref_attnum: i32,
    pub ref_name: String,
    pub ref_ident: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKey {
    pub name: String,
    pub ident: String,
    pub columns: Vec<ForeignColumn>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub name: String,
    pub command: String,
    pub permissive: bool,
    pub using_expr: Option<EvidenceHash>,
    pub check_expr: Option<EvidenceHash>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeReason {
    KnownBuiltinNotConverted,
    DomainNotConverted,
    EnumNotConverted,
    UnboundedOrWideNumeric,
    UnknownTypeOid,
}

impl TypeReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KnownBuiltinNotConverted => "known_builtin_not_converted",
            Self::DomainNotConverted => "domain_not_converted",
            Self::EnumNotConverted => "enum_not_converted",
            Self::UnboundedOrWideNumeric => "unbounded_or_wide_numeric",
            Self::UnknownTypeOid => "unknown_type_oid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub attnum: i32,
    pub name: String,
    pub ident: String,
    pub type_oid: i64,
    pub type_schema: String,
    pub type_name: String,
    pub typtype: String,
    pub typmod: i32,
    pub base_type_oid: Option<i64>,
    pub numeric_precision: Option<i32>,
    pub numeric_scale: Option<i32>,
    pub not_null: bool,
    pub collation_oid: i64,
    pub collation_name: Option<String>,
    pub identity: String,
    pub generated: String,
    pub default_expr: Option<EvidenceHash>,
    pub comment: Option<EvidenceHash>,
    /// Server enum labels. Used for `pg-catalog-hash/v1` and then reduced to a
    /// count plus a hash in the draft. The text is not copied into OOS fields.
    pub enum_labels: Vec<String>,
    pub domain_checks: Vec<EvidenceHash>,
    pub reason: TypeReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub schema: String,
    pub schema_ident: String,
    pub name: String,
    pub name_ident: String,
    pub relkind: String,
    pub replica_identity: String,
    pub rls_enabled: bool,
    pub rls_forced: bool,
    pub comment: Option<EvidenceHash>,
    pub columns: Vec<Column>,
    pub primary_key: Vec<KeyColumn>,
    pub primary_key_nulls_not_distinct: Option<bool>,
    pub unique_keys: Vec<UniqueKey>,
    pub foreign_keys: Vec<ForeignKey>,
    pub policies: Vec<Policy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    pub source_id: String,
    pub database: String,
    pub captured_at: String,
    pub schema_hash: String,
    pub tables: Vec<Table>,
}

pub fn load(value: &Value) -> Result<Catalog, DraftError> {
    let provided = value
        .get("schema_hash")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let catalog = parse_catalog(value)?;
    if catalog.schema_hash != provided {
        return Err(DraftError::new(
            "hash.schema",
            "/catalog/schema_hash",
            "schema_hash does not match pg-catalog-hash/v1",
        ));
    }
    Ok(catalog)
}

pub(crate) fn parse_catalog(value: &Value) -> Result<Catalog, DraftError> {
    let parsed: CatalogIn = serde_json::from_value(value.clone()).map_err(map_de)?;
    validate(parsed)
}

pub fn schema_hash(tables: &[Table]) -> String {
    sha256_hex(schema_preimage(tables).as_bytes())
}

pub fn schema_preimage(tables: &[Table]) -> String {
    let mut ordered: Vec<&Table> = tables.iter().collect();
    ordered.sort_by(|left, right| (&left.schema, &left.name).cmp(&(&right.schema, &right.name)));
    let mut buf = String::from("{\"tables\":[");
    for (index, table) in ordered.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        write_table(&mut buf, table);
    }
    buf.push_str("]}");
    buf
}

fn write_table(buf: &mut String, table: &Table) {
    buf.push('{');
    write_str(buf, "schema", &table.schema);
    buf.push(',');
    write_str(buf, "name", &table.name);
    buf.push(',');
    write_str(buf, "relkind", &table.relkind);
    buf.push(',');
    write_str(buf, "replica_identity", &table.replica_identity);
    buf.push(',');
    write_bool(buf, "rls", table.rls_enabled);
    buf.push(',');
    write_bool(buf, "force_rls", table.rls_forced);
    buf.push_str(",\"columns\":[");
    let mut columns = table.columns.clone();
    columns.sort_by_key(|column| column.attnum);
    for (index, column) in columns.iter().enumerate() {
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
        write_str(buf, "name", &key.name);
        buf.push_str(",\"columns\":[");
        write_key_columns(buf, &key.columns);
        buf.push_str("],");
        write_bool(buf, "nulls_not_distinct", key.nulls_not_distinct);
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
        write_str(buf, "name", &key.name);
        buf.push_str(",\"columns\":[");
        for (col_index, column) in key.columns.iter().enumerate() {
            if col_index > 0 {
                buf.push(',');
            }
            buf.push('{');
            write_i32(buf, "attnum", column.attnum);
            buf.push(',');
            write_str(buf, "name", &column.name);
            buf.push(',');
            write_str(buf, "ref_schema", &column.ref_schema);
            buf.push(',');
            write_str(buf, "ref_table", &column.ref_table);
            buf.push(',');
            write_i32(buf, "ref_attnum", column.ref_attnum);
            buf.push(',');
            write_str(buf, "ref_name", &column.ref_name);
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
        write_str(buf, "name", &policy.name);
        buf.push(',');
        write_str(buf, "command", &policy.command);
        buf.push(',');
        write_bool(buf, "permissive", policy.permissive);
        buf.push(',');
        write_evidence(buf, "using_sha256", policy.using_expr.as_ref());
        buf.push(',');
        write_evidence(buf, "check_sha256", policy.check_expr.as_ref());
        buf.push('}');
    }
    buf.push_str("]}");
}

fn write_column(buf: &mut String, column: &Column) {
    buf.push('{');
    write_i32(buf, "attnum", column.attnum);
    buf.push(',');
    write_str(buf, "name", &column.name);
    buf.push(',');
    write_i64(buf, "type_oid", column.type_oid);
    buf.push(',');
    write_i32(buf, "typmod", column.typmod);
    buf.push(',');
    write_bool(buf, "not_null", column.not_null);
    buf.push(',');
    write_i64(buf, "collation_oid", column.collation_oid);
    buf.push(',');
    write_str(buf, "typtype", &column.typtype);
    buf.push(',');
    write_opt_i64(buf, "base_type_oid", column.base_type_oid);
    buf.push(',');
    write_str(buf, "identity", &column.identity);
    buf.push(',');
    write_str(buf, "generated", &column.generated);
    buf.push(',');
    write_evidence(buf, "default_sha256", column.default_expr.as_ref());
    buf.push_str(",\"enum_labels\":[");
    for (index, label) in column.enum_labels.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        push_string(buf, label);
    }
    buf.push_str("],\"domain_checks\":[");
    for (index, check) in column.domain_checks.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        push_string(buf, &check.sha256);
    }
    buf.push_str("]}");
}

fn write_key_columns(buf: &mut String, columns: &[KeyColumn]) {
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_i32(buf, "attnum", column.attnum);
        buf.push(',');
        write_str(buf, "name", &column.name);
        buf.push('}');
    }
}

fn write_str(buf: &mut String, name: &str, value: &str) {
    push_string(buf, name);
    buf.push(':');
    push_string(buf, value);
}

fn write_bool(buf: &mut String, name: &str, value: bool) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(if value { "true" } else { "false" });
}

fn write_i32(buf: &mut String, name: &str, value: i32) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_i64(buf: &mut String, name: &str, value: i64) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_opt_i64(buf: &mut String, name: &str, value: Option<i64>) {
    push_string(buf, name);
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

fn write_evidence(buf: &mut String, name: &str, value: Option<&EvidenceHash>) {
    push_string(buf, name);
    buf.push(':');
    match value {
        Some(evidence) => push_string(buf, &evidence.sha256),
        None => buf.push_str("null"),
    }
}

fn map_de(err: serde_json::Error) -> DraftError {
    let text = err.to_string();
    let code = if text.contains("unknown field") {
        "field.unknown"
    } else if text.contains("missing field") {
        "field.missing"
    } else if text.contains("duplicate field") {
        "field.duplicate"
    } else {
        "schema.invalid"
    };
    DraftError::new(
        code,
        "/catalog",
        "catalog evidence is not oo.source.pgsql/v1",
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogIn {
    protocol: String,
    core_contract: String,
    source_id: String,
    database: String,
    captured_at: String,
    schema_hash: String,
    hash_algorithm: String,
    ident_algorithm: String,
    tables: Vec<TableIn>,
    type_conversion: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TableIn {
    schema: String,
    schema_ident: String,
    name: String,
    name_ident: String,
    relkind: String,
    replica_identity: String,
    rls_enabled: bool,
    rls_forced: bool,
    owner: String,
    comment: Option<EvidenceIn>,
    columns: Vec<ColumnIn>,
    primary_key: Vec<KeyColumnIn>,
    primary_key_nulls_not_distinct: Option<bool>,
    unique_keys: Vec<UniqueIn>,
    foreign_keys: Vec<ForeignIn>,
    policies: Vec<PolicyIn>,
    privileges: Vec<PrivilegeIn>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceIn {
    byte_len: u64,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyColumnIn {
    attnum: i32,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UniqueIn {
    name: String,
    columns: Vec<KeyColumnIn>,
    nulls_not_distinct: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ForeignColumnIn {
    attnum: i32,
    name: String,
    ref_schema: String,
    ref_table: String,
    ref_attnum: i32,
    ref_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ForeignIn {
    name: String,
    columns: Vec<ForeignColumnIn>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyIn {
    name: String,
    command: String,
    permissive: bool,
    using_expr: Option<EvidenceIn>,
    check_expr: Option<EvidenceIn>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivilegeIn {
    grantee: String,
    privilege: String,
    grantable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeSupportIn {
    converted: bool,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnIn {
    attnum: i32,
    name: String,
    name_ident: String,
    type_oid: i64,
    type_schema: String,
    type_name: String,
    typtype: String,
    typmod: i32,
    base_type_oid: Option<i64>,
    numeric_precision: Option<i32>,
    numeric_scale: Option<i32>,
    not_null: bool,
    collation_oid: i64,
    collation_name: Option<String>,
    identity: String,
    generated: String,
    default_expr: Option<EvidenceIn>,
    comment: Option<EvidenceIn>,
    enum_labels: Vec<String>,
    domain_checks: Vec<EvidenceIn>,
    type_support: TypeSupportIn,
    current_privileges: ColumnPrivilegesIn,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnPrivilegesIn {
    select: Option<bool>,
    insert: Option<bool>,
    update: Option<bool>,
}

fn validate(input: CatalogIn) -> Result<Catalog, DraftError> {
    let path = "/catalog";
    if input.protocol != PROTOCOL {
        return Err(DraftError::new(
            "version.unsupported",
            path,
            "catalog protocol is not oo.source.pgsql/v1",
        ));
    }
    if input.core_contract != CORE_CONTRACT
        || input.hash_algorithm != HASH_ALGORITHM
        || input.ident_algorithm != IDENT_ALGORITHM
        || input.type_conversion != "unsupported"
    {
        return Err(DraftError::new(
            "version.unsupported",
            path,
            "catalog hash, ident, contract, or type conversion is not supported",
        ));
    }
    if !is_sha256(&input.schema_hash) {
        return Err(DraftError::new(
            "hash.schema",
            "/catalog/schema_hash",
            "schema_hash is not lowercase sha256",
        ));
    }
    if !valid_captured_at(&input.captured_at) {
        return Err(DraftError::new(
            "field.invalid",
            "/catalog/captured_at",
            "captured_at is not a 27-character UTC timestamp",
        ));
    }
    check_token(&input.source_id, "/catalog/source_id")?;
    check_pg_name(&input.database, "/catalog/database")?;
    if input.tables.len() > MAX_TABLES {
        return Err(DraftError::new(
            "budget.tables",
            "/catalog/tables",
            "table count exceeds the draft budget",
        ));
    }
    if input.tables.iter().any(|table| {
        input
            .tables
            .iter()
            .filter(|other| other.schema == table.schema && other.name == table.name)
            .count()
            > 1
    }) {
        return Err(DraftError::new(
            "duplicate.table",
            "/catalog/tables",
            "duplicate schema and table name",
        ));
    }
    let mut schema_names = Vec::new();
    for table in &input.tables {
        if !schema_names.contains(&table.schema) {
            schema_names.push(table.schema.clone());
        }
    }
    assign_idents(&schema_names, "/catalog/tables")?;
    let mut grouped: Vec<(String, Vec<String>)> = Vec::new();
    for table in &input.tables {
        if let Some((_, names)) = grouped
            .iter_mut()
            .find(|(schema, _)| schema == &table.schema)
        {
            names.push(table.name.clone());
        } else {
            grouped.push((table.schema.clone(), vec![table.name.clone()]));
        }
    }
    for (_, names) in &grouped {
        assign_idents(names, "/catalog/tables")?;
    }
    let mut tables = Vec::with_capacity(input.tables.len());
    for table in input.tables {
        tables.push(validate_table(table)?);
    }
    tables.sort_by(|left, right| (&left.schema, &left.name).cmp(&(&right.schema, &right.name)));
    let schema_hash = schema_hash(&tables);
    Ok(Catalog {
        source_id: input.source_id,
        database: input.database,
        captured_at: input.captured_at,
        schema_hash,
        tables,
    })
}

fn validate_table(input: TableIn) -> Result<Table, DraftError> {
    let path = "/catalog/tables";
    check_pg_name(&input.schema, "/catalog/tables/schema")?;
    check_pg_name(&input.name, "/catalog/tables/name")?;
    let schema_ident = stable_ident(&input.schema, path)?;
    let name_ident = stable_ident(&input.name, path)?;
    if input.schema_ident != schema_ident || input.name_ident != name_ident {
        return Err(DraftError::new(
            "ident.mismatch",
            path,
            "provided stable identifier does not match pgident/v1",
        ));
    }
    check_mark(&input.relkind, "/catalog/tables/relkind")?;
    check_mark(&input.replica_identity, "/catalog/tables/replica_identity")?;
    check_pg_name(&input.owner, "/catalog/tables/owner")?;
    if input.columns.len() > MAX_COLUMNS {
        return Err(DraftError::new(
            "budget.columns",
            "/catalog/tables/columns",
            "column count exceeds the draft budget",
        ));
    }
    let key_count = input.unique_keys.len()
        + input.foreign_keys.len()
        + usize::from(!input.primary_key.is_empty());
    if key_count > MAX_KEYS {
        return Err(DraftError::new(
            "budget.keys",
            "/catalog/tables",
            "key count exceeds the draft budget",
        ));
    }
    if input.policies.len() > MAX_POLICIES {
        return Err(DraftError::new(
            "budget.policies",
            "/catalog/tables/policies",
            "policy count exceeds the draft budget",
        ));
    }
    if input.privileges.len() > MAX_PRIVILEGES {
        return Err(DraftError::new(
            "budget.privileges",
            "/catalog/tables/privileges",
            "privilege count exceeds the draft budget",
        ));
    }
    for privilege in &input.privileges {
        check_pg_name(&privilege.grantee, "/catalog/tables/privileges")?;
        check_pg_name(&privilege.privilege, "/catalog/tables/privileges")?;
        let _ = privilege.grantable;
    }
    let mut columns = Vec::with_capacity(input.columns.len());
    let mut seen_attnum = Vec::new();
    let mut seen_names = Vec::new();
    for column in input.columns {
        if !(1..=1600).contains(&column.attnum) {
            return Err(DraftError::new(
                "field.invalid",
                "/catalog/tables/columns/attnum",
                "attnum is outside 1..=1600",
            ));
        }
        if seen_attnum.contains(&column.attnum) {
            return Err(DraftError::new(
                "duplicate.attnum",
                "/catalog/tables/columns",
                "duplicate column attnum",
            ));
        }
        if seen_names.iter().any(|name: &String| name == &column.name) {
            return Err(DraftError::new(
                "duplicate.column",
                "/catalog/tables/columns",
                "duplicate column name",
            ));
        }
        seen_attnum.push(column.attnum);
        seen_names.push(column.name.clone());
        columns.push(validate_column(column)?);
    }
    assign_idents(&seen_names, "/catalog/tables/columns")?;
    columns.sort_by_key(|column| column.attnum);
    if input.primary_key.is_empty() {
        if input.primary_key_nulls_not_distinct.is_some() {
            return Err(DraftError::new(
                "field.invalid",
                "/catalog/tables/primary_key_nulls_not_distinct",
                "nulls-not-distinct requires a primary key",
            ));
        }
    } else if input.primary_key_nulls_not_distinct.is_none() {
        return Err(DraftError::new(
            "field.invalid",
            "/catalog/tables/primary_key_nulls_not_distinct",
            "a primary key requires nulls-not-distinct",
        ));
    }
    let primary_key = if input.primary_key.is_empty() {
        Vec::new()
    } else {
        validate_key_columns(&input.primary_key, &columns, "/catalog/tables/primary_key")?
    };
    let mut key_names = Vec::new();
    let mut unique_keys = Vec::with_capacity(input.unique_keys.len());
    for key in input.unique_keys {
        let ident = validate_constraint_name(&key.name, &mut key_names)?;
        unique_keys.push(UniqueKey {
            ident,
            columns: validate_key_columns(&key.columns, &columns, "/catalog/tables/unique_keys")?,
            nulls_not_distinct: key.nulls_not_distinct,
            name: key.name,
        });
    }
    let mut foreign_keys = Vec::with_capacity(input.foreign_keys.len());
    for key in input.foreign_keys {
        let ident = validate_constraint_name(&key.name, &mut key_names)?;
        if key.columns.is_empty() {
            return Err(DraftError::new(
                "key.empty",
                "/catalog/tables/foreign_keys",
                "key has no columns",
            ));
        }
        if key.columns.len() > MAX_KEY_COLUMNS {
            return Err(DraftError::new(
                "budget.key_columns",
                "/catalog/tables/foreign_keys",
                "foreign key column count exceeds the draft budget",
            ));
        }
        let mut pairs = Vec::with_capacity(key.columns.len());
        let mut ref_schema = None;
        let mut ref_table = None;
        let mut seen_local = MemberSeen::default();
        for column in key.columns {
            if ref_schema
                .as_ref()
                .is_some_and(|schema| schema != &column.ref_schema)
                || ref_table
                    .as_ref()
                    .is_some_and(|table| table != &column.ref_table)
            {
                return Err(DraftError::new(
                    "field.invalid",
                    "/catalog/tables/foreign_keys",
                    "one foreign key references one table",
                ));
            }
            ref_schema = Some(column.ref_schema.clone());
            ref_table = Some(column.ref_table.clone());
            let local = take_key_member(
                column.attnum,
                &column.name,
                &columns,
                &mut seen_local,
                "/catalog/tables/foreign_keys",
                "foreign key column is not in the table",
            )?;
            check_pg_name(&column.ref_schema, "/catalog/tables/foreign_keys")?;
            check_pg_name(&column.ref_table, "/catalog/tables/foreign_keys")?;
            check_pg_name(&column.ref_name, "/catalog/tables/foreign_keys")?;
            if !(1..=1600).contains(&column.ref_attnum) {
                return Err(DraftError::new(
                    "field.invalid",
                    "/catalog/tables/foreign_keys",
                    "referenced attnum is outside 1..=1600",
                ));
            }
            pairs.push(ForeignColumn {
                attnum: column.attnum,
                name: column.name,
                ident: local.ident.clone(),
                ref_schema: column.ref_schema.clone(),
                ref_schema_ident: stable_ident(&column.ref_schema, "/catalog/tables/foreign_keys")?,
                ref_table: column.ref_table.clone(),
                ref_table_ident: stable_ident(&column.ref_table, "/catalog/tables/foreign_keys")?,
                ref_attnum: column.ref_attnum,
                ref_name: column.ref_name.clone(),
                ref_ident: stable_ident(&column.ref_name, "/catalog/tables/foreign_keys")?,
            });
        }
        foreign_keys.push(ForeignKey {
            name: key.name,
            ident,
            columns: pairs,
        });
    }
    unique_keys.sort_by(|left, right| left.name.cmp(&right.name));
    foreign_keys.sort_by(|left, right| left.name.cmp(&right.name));
    let mut policies = Vec::with_capacity(input.policies.len());
    let mut policy_names = Vec::new();
    for policy in input.policies {
        check_pg_name(&policy.name, "/catalog/tables/policies")?;
        if policy_names
            .iter()
            .any(|name: &String| name == &policy.name)
        {
            return Err(DraftError::new(
                "duplicate.key",
                "/catalog/tables/policies",
                "duplicate policy name",
            ));
        }
        policy_names.push(policy.name.clone());
        if policy.command.len() != 1
            || !policy
                .command
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '*')
        {
            return Err(DraftError::new(
                "field.invalid",
                "/catalog/tables/policies",
                "policy command is not one catalog character",
            ));
        }
        policies.push(Policy {
            name: policy.name,
            command: policy.command,
            permissive: policy.permissive,
            using_expr: optional_evidence(policy.using_expr, "/catalog/tables/policies")?,
            check_expr: optional_evidence(policy.check_expr, "/catalog/tables/policies")?,
        });
    }
    policies.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(Table {
        schema: input.schema,
        schema_ident,
        name: input.name,
        name_ident,
        relkind: input.relkind,
        replica_identity: input.replica_identity,
        rls_enabled: input.rls_enabled,
        rls_forced: input.rls_forced,
        comment: optional_evidence(input.comment, "/catalog/tables/comment")?,
        columns,
        primary_key,
        primary_key_nulls_not_distinct: input.primary_key_nulls_not_distinct,
        unique_keys,
        foreign_keys,
        policies,
    })
}

fn validate_column(input: ColumnIn) -> Result<Column, DraftError> {
    let path = "/catalog/tables/columns";
    check_pg_name(&input.name, path)?;
    check_pg_name(&input.type_schema, path)?;
    check_pg_name(&input.type_name, path)?;
    let ident = stable_ident(&input.name, path)?;
    if input.name_ident != ident {
        return Err(DraftError::new(
            "ident.mismatch",
            path,
            "provided stable identifier does not match pgident/v1",
        ));
    }
    if input.type_oid < 1 || input.type_oid > i64::from(u32::MAX) {
        return Err(DraftError::new(
            "field.invalid",
            path,
            "type oid is outside the PostgreSQL oid range",
        ));
    }
    if input.typmod < -1 {
        return Err(DraftError::new("field.invalid", path, "typmod is below -1"));
    }
    if input.typtype.len() != 1 || !input.typtype.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return Err(DraftError::new(
            "field.invalid",
            path,
            "typtype is not one letter",
        ));
    }
    if !matches!(input.identity.as_str(), "" | "a" | "d")
        || !matches!(input.generated.as_str(), "" | "s")
    {
        return Err(DraftError::new(
            "field.invalid",
            path,
            "identity or generated marker is not a catalog value",
        ));
    }
    if input.collation_oid < 0 || input.collation_oid > i64::from(u32::MAX) {
        return Err(DraftError::new(
            "field.invalid",
            path,
            "collation oid is outside the PostgreSQL oid range",
        ));
    }
    match (input.collation_oid == 0, input.collation_name.as_deref()) {
        (true, None) => {}
        (false, Some(name)) => check_pg_name(name, path)?,
        _ => {
            return Err(DraftError::new(
                "field.invalid",
                path,
                "collation name does not match collation oid",
            ));
        }
    }
    if input.typtype == "d" {
        match input.base_type_oid {
            Some(oid) if (1..=i64::from(u32::MAX)).contains(&oid) => {}
            _ => {
                return Err(DraftError::new(
                    "field.invalid",
                    path,
                    "a domain requires a base type oid",
                ));
            }
        }
    } else if input.base_type_oid.is_some() {
        return Err(DraftError::new(
            "field.invalid",
            path,
            "base type oid is only valid for a domain",
        ));
    }
    let parts = if input.type_name == "numeric" {
        numeric_parts(input.typmod)
    } else {
        None
    };
    if input.type_name == "numeric" {
        if input.numeric_precision != parts.map(|(precision, _)| precision)
            || input.numeric_scale != parts.map(|(_, scale)| scale)
        {
            return Err(DraftError::new(
                "field.invalid",
                path,
                "numeric precision or scale does not match typmod",
            ));
        }
    } else if input.numeric_precision.is_some() || input.numeric_scale.is_some() {
        return Err(DraftError::new(
            "field.invalid",
            path,
            "precision and scale are only valid for numeric",
        ));
    }
    let reason = classify(&input.typtype, &input.type_name, input.typmod);
    if input.type_support.reason != reason.as_str() {
        return Err(DraftError::new(
            "type.mismatch",
            path,
            "type_support.reason does not match the physical type",
        ));
    }
    if input.type_support.converted {
        return Err(DraftError::new(
            "type.converted",
            path,
            "type conversion is not supported",
        ));
    }
    let _ = (
        input.current_privileges.select,
        input.current_privileges.insert,
        input.current_privileges.update,
    );
    if input.enum_labels.len() > MAX_ENUM_LABELS {
        return Err(DraftError::new(
            "budget.enum_labels",
            path,
            "enum label count exceeds the draft budget",
        ));
    }
    if input.typtype == "e" {
        for label in &input.enum_labels {
            check_pg_name(label, path)?;
        }
    } else if !input.enum_labels.is_empty() {
        return Err(DraftError::new(
            "field.invalid",
            path,
            "enum labels require an enum type",
        ));
    }
    if input.domain_checks.len() > MAX_DOMAIN_CHECKS {
        return Err(DraftError::new(
            "budget.domain_checks",
            path,
            "domain check count exceeds the draft budget",
        ));
    }
    if input.typtype != "d" && !input.domain_checks.is_empty() {
        return Err(DraftError::new(
            "field.invalid",
            path,
            "domain checks require a domain",
        ));
    }
    let mut domain_checks = Vec::with_capacity(input.domain_checks.len());
    for check in input.domain_checks {
        domain_checks.push(require_evidence(check, path)?);
    }
    Ok(Column {
        attnum: input.attnum,
        name: input.name,
        ident,
        type_oid: input.type_oid,
        type_schema: input.type_schema,
        type_name: input.type_name,
        typtype: input.typtype,
        typmod: input.typmod,
        base_type_oid: input.base_type_oid,
        numeric_precision: input.numeric_precision,
        numeric_scale: input.numeric_scale,
        not_null: input.not_null,
        collation_oid: input.collation_oid,
        collation_name: input.collation_name,
        identity: input.identity,
        generated: input.generated,
        default_expr: optional_evidence(input.default_expr, path)?,
        comment: optional_evidence(input.comment, path)?,
        enum_labels: input.enum_labels,
        domain_checks,
        reason,
    })
}

#[derive(Default)]
struct MemberSeen {
    attnums: Vec<i32>,
    names: Vec<String>,
    idents: Vec<String>,
}

fn take_key_member(
    attnum: i32,
    name: &str,
    table_columns: &[Column],
    seen: &mut MemberSeen,
    path: &'static str,
    missing: &'static str,
) -> Result<KeyColumn, DraftError> {
    let resolved = table_columns
        .iter()
        .find(|item| item.attnum == attnum && item.name == name);
    let ident = if let Some(found) = resolved {
        found.ident.clone()
    } else {
        let seen_raw = seen.attnums.contains(&attnum) || seen.names.iter().any(|item| item == name);
        let candidate = match stable_ident(name, path) {
            Ok(ident) => ident,
            Err(_) if seen_raw => String::new(),
            Err(_) => return Err(DraftError::new(path_code(path), path, missing)),
        };
        let repeated = seen_raw
            || (!candidate.is_empty() && seen.idents.iter().any(|item| item == &candidate));
        if !repeated {
            return Err(DraftError::new(path_code(path), path, missing));
        }
        candidate
    };
    seen.attnums.push(attnum);
    seen.names.push(name.to_string());
    seen.idents.push(ident.clone());
    Ok(KeyColumn {
        attnum,
        name: name.to_string(),
        ident,
    })
}

fn validate_key_columns(
    columns: &[KeyColumnIn],
    table_columns: &[Column],
    path: &'static str,
) -> Result<Vec<KeyColumn>, DraftError> {
    if columns.len() > MAX_KEY_COLUMNS {
        return Err(DraftError::new(
            "budget.key_columns",
            path,
            "key column count exceeds the draft budget",
        ));
    }
    let mut seen = MemberSeen::default();
    let mut out = Vec::with_capacity(columns.len());
    for column in columns {
        out.push(take_key_member(
            column.attnum,
            &column.name,
            table_columns,
            &mut seen,
            path,
            "key column is not in the table",
        )?);
    }
    if columns.is_empty() {
        return Err(DraftError::new("key.empty", path, "key has no columns"));
    }
    Ok(out)
}

pub(crate) fn reject_repeated_key_members(catalog: &Catalog) -> Result<(), DraftError> {
    for table in &catalog.tables {
        reject_repeated_members(&table.primary_key, "/catalog/tables/primary_key")?;
        for key in &table.unique_keys {
            reject_repeated_members(&key.columns, "/catalog/tables/unique_keys")?;
        }
        for key in &table.foreign_keys {
            reject_repeated_members(
                &key.columns
                    .iter()
                    .map(|column| KeyColumn {
                        attnum: column.attnum,
                        name: column.name.clone(),
                        ident: column.ident.clone(),
                    })
                    .collect::<Vec<_>>(),
                "/catalog/tables/foreign_keys",
            )?;
            reject_repeated_members(
                &key.columns
                    .iter()
                    .map(|column| KeyColumn {
                        attnum: column.ref_attnum,
                        name: column.ref_name.clone(),
                        ident: column.ref_ident.clone(),
                    })
                    .collect::<Vec<_>>(),
                "/catalog/tables/foreign_keys",
            )?;
        }
    }
    Ok(())
}

fn reject_repeated_members(columns: &[KeyColumn], path: &'static str) -> Result<(), DraftError> {
    let mut attnums = Vec::new();
    let mut names: Vec<&str> = Vec::new();
    let mut idents: Vec<&str> = Vec::new();
    for column in columns {
        if attnums.contains(&column.attnum)
            || names.contains(&column.name.as_str())
            || idents.contains(&column.ident.as_str())
        {
            return Err(DraftError::new(
                "duplicate.key_member",
                path,
                "repeated key member",
            ));
        }
        attnums.push(column.attnum);
        names.push(column.name.as_str());
        idents.push(column.ident.as_str());
    }
    Ok(())
}

fn path_code(path: &str) -> &'static str {
    if path.ends_with("primary_key")
        || path.ends_with("unique_keys")
        || path.ends_with("foreign_keys")
    {
        "key.unknown_column"
    } else {
        "field.invalid"
    }
}

fn validate_constraint_name(name: &str, seen: &mut Vec<String>) -> Result<String, DraftError> {
    check_pg_name(name, "/catalog/tables")?;
    if name == "primary_key" {
        return Err(DraftError::new(
            "key.reserved_name",
            "/catalog/tables",
            "constraint name primary_key is reserved for the selection token",
        ));
    }
    if seen.iter().any(|item| item == name) {
        return Err(DraftError::new(
            "duplicate.key",
            "/catalog/tables",
            "duplicate constraint name",
        ));
    }
    seen.push(name.to_string());
    let ident = stable_ident(name, "/catalog/tables")?;
    let originals = seen.clone();
    assign_idents(&originals, "/catalog/tables")?;
    Ok(ident)
}

fn optional_evidence(
    value: Option<EvidenceIn>,
    path: &str,
) -> Result<Option<EvidenceHash>, DraftError> {
    value.map(|item| require_evidence(item, path)).transpose()
}

fn require_evidence(value: EvidenceIn, path: &str) -> Result<EvidenceHash, DraftError> {
    if !is_sha256(&value.sha256) || value.byte_len > MAX_UNTRUSTED_BYTES {
        return Err(DraftError::new(
            "evidence.invalid",
            path,
            "untrusted evidence hash or length is not valid",
        ));
    }
    if value.byte_len == 0 && value.sha256 != EMPTY_SHA256 {
        return Err(DraftError::new(
            "evidence.invalid",
            path,
            "empty evidence must use the empty sha256",
        ));
    }
    Ok(EvidenceHash {
        byte_len: value.byte_len,
        sha256: value.sha256,
    })
}

pub fn numeric_parts(typmod: i32) -> Option<(i32, i32)> {
    if typmod < 0 {
        return None;
    }
    let adjusted = typmod - 4;
    let precision = (adjusted >> 16) & 0xFFFF;
    let scale = adjusted & 0xFFFF;
    Some((precision, scale))
}

pub fn classify(typtype: &str, type_name: &str, typmod: i32) -> TypeReason {
    if typtype == "d" {
        return TypeReason::DomainNotConverted;
    }
    if typtype == "e" {
        return TypeReason::EnumNotConverted;
    }
    if type_name == "numeric" {
        match numeric_parts(typmod) {
            None => return TypeReason::UnboundedOrWideNumeric,
            Some((precision, _)) if precision > 38 => return TypeReason::UnboundedOrWideNumeric,
            Some(_) => {}
        }
    }
    if matches!(typtype, "b" | "p" | "r" | "m" | "c") {
        return TypeReason::KnownBuiltinNotConverted;
    }
    TypeReason::UnknownTypeOid
}

fn check_token(value: &str, path: &str) -> Result<(), DraftError> {
    let bytes = value.as_bytes();
    if (1..=63).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        })
    {
        Ok(())
    } else {
        Err(DraftError::new(
            "field.invalid",
            path,
            "source id is not a lowercase token",
        ))
    }
}

fn check_pg_name(value: &str, path: &str) -> Result<(), DraftError> {
    if value.is_empty() || value.as_bytes().contains(&0) {
        return Err(DraftError::new(
            "ident.empty",
            path,
            "identifier is empty or contains NUL",
        ));
    }
    if value.len() > MAX_NAME_BYTES {
        return Err(DraftError::new(
            "budget.name_bytes",
            path,
            "identifier exceeds the PostgreSQL byte ceiling",
        ));
    }
    if value.chars().any(|ch| (ch as u32) < 0x20 || ch == '\u{7f}') {
        return Err(DraftError::new(
            "ident.control",
            path,
            "identifier contains an ASCII control character",
        ));
    }
    Ok(())
}

fn check_mark(value: &str, path: &str) -> Result<(), DraftError> {
    if value.len() == 1 && value.chars().all(|ch| ch.is_ascii_alphabetic()) {
        Ok(())
    } else {
        Err(DraftError::new(
            "field.invalid",
            path,
            "catalog marker is not one letter",
        ))
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_captured_at(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 27 || bytes[10] != b'T' || bytes[19] != b'.' || bytes[26] != b'Z' {
        return false;
    }
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[13] != b':' || bytes[16] != b':' {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| {
        matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 26) || byte.is_ascii_digit()
    })
}

const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[cfg(test)]
mod tests {
    use super::*;

    fn column() -> Column {
        Column {
            attnum: 1,
            name: "id".to_string(),
            ident: "id".to_string(),
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
            reason: TypeReason::KnownBuiltinNotConverted,
        }
    }

    fn customer() -> Table {
        Table {
            schema: "public".to_string(),
            schema_ident: "public".to_string(),
            name: "customer".to_string(),
            name_ident: "customer".to_string(),
            relkind: "r".to_string(),
            replica_identity: "d".to_string(),
            rls_enabled: false,
            rls_forced: false,
            comment: None,
            columns: vec![column()],
            primary_key: vec![KeyColumn {
                attnum: 1,
                name: "id".to_string(),
                ident: "id".to_string(),
            }],
            primary_key_nulls_not_distinct: Some(false),
            unique_keys: Vec::new(),
            foreign_keys: Vec::new(),
            policies: Vec::new(),
        }
    }

    #[test]
    fn empty_and_one_column_hashes_match_independent_vectors() {
        assert_eq!(schema_preimage(&[]), r#"{"tables":[]}"#);
        assert_eq!(schema_hash(&[]), EMPTY_SCHEMA_HASH);
        let expected = r#"{"tables":[{"schema":"public","name":"customer","relkind":"r","replica_identity":"d","rls":false,"force_rls":false,"columns":[{"attnum":1,"name":"id","type_oid":23,"typmod":-1,"not_null":true,"collation_oid":0,"typtype":"b","base_type_oid":null,"identity":"","generated":"","default_sha256":null,"enum_labels":[],"domain_checks":[]}],"primary_key":[{"attnum":1,"name":"id"}],"primary_key_nulls_not_distinct":false,"unique_keys":[],"foreign_keys":[],"policies":[]}]}"#;
        assert_eq!(schema_preimage(&[customer()]), expected);
        assert_eq!(
            schema_hash(&[customer()]),
            "60f7939195cc539644d11132ae4a604c3b025167f1fe657d121167feb8463499"
        );
    }

    #[test]
    fn wide_numeric_and_domain_are_not_coerced() {
        assert_eq!(
            classify("b", "numeric", -1),
            TypeReason::UnboundedOrWideNumeric
        );
        let wide = ((80 << 16) | 10) + 4;
        assert_eq!(numeric_parts(wide), Some((80, 10)));
        assert_eq!(
            classify("b", "numeric", wide),
            TypeReason::UnboundedOrWideNumeric
        );
        assert_eq!(
            classify("d", "numeric", wide),
            TypeReason::DomainNotConverted
        );
        assert_eq!(classify("e", "mood", -1), TypeReason::EnumNotConverted);
        assert_eq!(
            classify("b", "int4", -1),
            TypeReason::KnownBuiltinNotConverted
        );
        assert_eq!(classify("x", "mystery", -1), TypeReason::UnknownTypeOid);
    }
}
