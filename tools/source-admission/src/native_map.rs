//! Engine-scoped grant interpretation for `admit`.
//!
//! The SQL closed sets in `model` stay unchanged. A name is read-only only for
//! an exact compat mode. This module does not admit a report.

use crate::model::{
    COMPAT_MODES, ClickHouseNative, GrantScope, PERMISSIONS, Permission, READ_ONLY_PERMISSIONS,
    known,
};

pub const MONGODB_NATIVE: &str = "mongodb-native";
pub const CLICKHOUSE_NATIVE: &str = "clickhouse-native";
pub const STARROCKS_NATIVE: &str = "starrocks-native";
pub const DORIS: &str = "doris";
pub const OPENSEARCH_NATIVE: &str = "opensearch-native";
pub const ORACLE: &str = "oracle";

pub const PRODUCT_COMPAT_MODES: &[&str] = &[
    MONGODB_NATIVE,
    CLICKHOUSE_NATIVE,
    STARROCKS_NATIVE,
    DORIS,
    OPENSEARCH_NATIVE,
];

/// MongoDB built-in roles other than the database role `read`.
/// Granted, each is `PERMISSION_NOT_READONLY`. The match is exact.
const MONGO_NOT_READONLY: &[&str] = &[
    "readWrite",
    "dbAdmin",
    "dbOwner",
    "userAdmin",
    "readAnyDatabase",
    "readWriteAnyDatabase",
    "userAdminAnyDatabase",
    "dbAdminAnyDatabase",
    "clusterAdmin",
    "clusterManager",
    "clusterMonitor",
    "hostManager",
    "backup",
    "restore",
    "root",
];

/// ClickHouse access types that write, change schema, or bypass the profile.
/// `SELECT` is not in this list. The setting name `readonly` is not in this list.
const CLICKHOUSE_NOT_READONLY: &[&str] = &[
    "INSERT",
    "ALTER",
    "CREATE",
    "DROP",
    "TRUNCATE",
    "OPTIMIZE",
    "DELETE",
    "UPDATE",
    "KILL QUERY",
    "ACCESS MANAGEMENT",
    "SYSTEM",
    "ALL",
    "CREATE TEMPORARY TABLE",
    "BACKUP",
    "RESTORE",
    "SOURCES",
];

/// OpenSearch action groups that include write, create-index, or every index action.
const OPENSEARCH_NOT_READONLY: &[&str] = &[
    "write",
    "crud",
    "create_index",
    "delete",
    "indices_all",
    "data_access",
    "all_access",
    "cluster_composite_ops",
];

pub const OPENSEARCH_SEARCH: &str = "indices:data/read/search";
pub const OPENSEARCH_MGET: &str = "indices:data/read/mget";

/// Oracle system and object privileges that are not an AS OF read.
/// `EXECUTE` stays in the SQL not-read-only set. `GRANT EXECUTE` is on a
/// package, so `SYS.DBMS_FLASHBACK` also covers `TRANSACTION_BACKOUT`.
const ORACLE_NOT_READONLY: &[&str] = &[
    "ALTER",
    "FLASHBACK ANY TABLE",
    "SELECT ANY TABLE",
    "SELECT ANY DICTIONARY",
    "SELECT ANY TRANSACTION",
    "EXECUTE ANY PROCEDURE",
    "LOGMINING",
];

/// Privileges that, with object `FLASHBACK`, complete `FLASHBACK TABLE`.
const FLASHBACK_TABLE_WRITES: &[&str] = &["INSERT", "UPDATE", "DELETE", "ALTER", "TRUNCATE"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeGrant {
    ReadOnly,
    NotReadOnly,
    /// Recognized and not a write. It does not satisfy the read requirement.
    Companion,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeFault {
    Ok,
    Scope,
    Observed,
}

pub fn compat_recognized(mode: &str) -> bool {
    known(COMPAT_MODES, mode) || known(PRODUCT_COMPAT_MODES, mode)
}

/// Product engine recorded on a capture binding.
///
/// One compat token maps to one engine name.
pub fn engine_for_compat(mode: &str) -> Option<&'static str> {
    match mode {
        DORIS => Some("doris"),
        STARROCKS_NATIVE => Some("starrocks"),
        CLICKHOUSE_NATIVE => Some("clickhouse"),
        OPENSEARCH_NATIVE => Some("opensearch"),
        MONGODB_NATIVE => Some("mongodb"),
        ORACLE => Some("oracle"),
        "postgres-native" => Some("postgresql"),
        "mariadb-native" => Some("mariadb"),
        "mysql-native" => Some("mysql"),
        "sqlserver" | "sqlserver-mode" => Some("sqlserver"),
        _ => None,
    }
}

/// Engines `oo-pipeline` `sealed_product` accepts.
/// All supported native capture engines share the sealed product contract.
pub fn seal_engine(engine: &str) -> bool {
    matches!(
        engine,
        "oracle" | "clickhouse" | "opensearch" | "doris" | "mongodb" | "starrocks" | "mariadb" | "mysql" | "postgresql" | "sqlserver"
    )
}

/// Classifies one permission name in a compat mode.
///
/// SQL modes, `starrocks-native`, and `doris` use the SQL permission table.
/// `mongodb-native` accepts the built-in role `read` and the built-in roles in
/// `MONGO_NOT_READONLY`. ClickHouse and OpenSearch classify names here; scope,
/// observation, and the ClickHouse profile are checked by `admit`.
pub fn interpret_native_grant(compat_mode: &str, name: &str) -> NativeGrant {
    match compat_mode {
        MONGODB_NATIVE => mongo_grant(name),
        CLICKHOUSE_NATIVE => clickhouse_grant(name),
        OPENSEARCH_NATIVE => opensearch_grant(name),
        ORACLE => oracle_name(name),
        "sqlserver" | "sqlserver-mode" if matches!(name, "VIEW CHANGE TRACKING" | "VIEW DEFINITION") => NativeGrant::ReadOnly,
        _ => sql_grant(name),
    }
}

/// `CREATE SESSION` connects. Object `FLASHBACK` is the AS OF privilege.
/// Scope and the write combination are checked by `admit`, not here.
fn oracle_name(name: &str) -> NativeGrant {
    match name {
        "CREATE SESSION" | "FLASHBACK" => NativeGrant::ReadOnly,
        _ if known(ORACLE_NOT_READONLY, name) => NativeGrant::NotReadOnly,
        _ => sql_grant(name),
    }
}

pub(crate) fn oracle_scope_used(scope: Option<&GrantScope>) -> bool {
    scope.is_some_and(|item| {
        item.database.is_some()
            || item.table.is_some()
            || item.index.is_some()
            || item.observed.is_some()
    })
}

pub(crate) fn oracle_object_scope(scope: Option<&GrantScope>) -> Result<(&str, &str), ()> {
    let Some(scope) = scope else {
        return Err(());
    };
    if scope.index.is_some() || scope.observed.is_some() {
        return Err(());
    }
    match (scope.database.as_deref(), scope.table.as_deref()) {
        (Some(schema), Some(table)) if oracle_ident(schema) && oracle_ident(table) => {
            Ok((schema, table))
        }
        _ => Err(()),
    }
}

pub(crate) fn oracle_session_granted(items: &[Permission]) -> bool {
    items.iter().any(|item| {
        item.name == "CREATE SESSION"
            && item.status == "known"
            && item.granted
            && !oracle_scope_used(item.scope.as_ref())
    })
}

pub(crate) fn oracle_flashback_write(items: &[Permission], schema: &str, table: &str) -> bool {
    items.iter().any(|item| {
        item.status == "known"
            && item.granted
            && known(FLASHBACK_TABLE_WRITES, &item.name)
            && oracle_write_covers(item, schema, table)
    })
}

fn oracle_write_covers(item: &Permission, schema: &str, table: &str) -> bool {
    if !oracle_scope_used(item.scope.as_ref()) {
        return true;
    }
    item.scope.as_ref().is_some_and(|scope| {
        scope.database.as_deref() == Some(schema) && scope.table.as_deref() == Some(table)
    })
}

fn oracle_ident(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 128
        && first.is_ascii_alphabetic()
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' || ch == '#')
}

pub(crate) fn clickhouse_profile_fault(profile: &ClickHouseNative) -> Result<(), &'static str> {
    if profile.readonly != "1" {
        return Err("readonly");
    }
    if !canonical_seconds(&profile.max_execution_time) {
        return Err("max_execution_time");
    }
    let quota = &profile.quota;
    if quota.duration_seconds == 0
        || quota.queries == 0
        || quota.errors == 0
        || quota.result_rows == 0
        || quota.read_rows == 0
        || quota.execution_time_seconds == 0
    {
        return Err("quota");
    }
    Ok(())
}

pub(crate) fn clickhouse_select_fault(scope: Option<&GrantScope>) -> ScopeFault {
    let Some(scope) = scope else {
        return ScopeFault::Scope;
    };
    if scope.index.is_some() || scope.observed.is_some() {
        return ScopeFault::Scope;
    }
    let Some(database) = scope.database.as_deref() else {
        return ScopeFault::Scope;
    };
    if !clickhouse_ident(database) {
        return ScopeFault::Scope;
    }
    match scope.table.as_deref() {
        None => ScopeFault::Ok,
        Some(table) if clickhouse_ident(table) => ScopeFault::Ok,
        Some(_) => ScopeFault::Scope,
    }
}

pub(crate) fn clickhouse_companion_ok(scope: Option<&GrantScope>) -> bool {
    let Some(scope) = scope else {
        return true;
    };
    if scope.index.is_some() || scope.observed.is_some() {
        return false;
    }
    scope.database.as_deref().is_none_or(clickhouse_ident)
        && scope.table.as_deref().is_none_or(clickhouse_ident)
}

pub(crate) fn opensearch_read_fault(scope: Option<&GrantScope>) -> ScopeFault {
    let Some(scope) = scope else {
        return ScopeFault::Scope;
    };
    if scope.database.is_some() || scope.table.is_some() {
        return ScopeFault::Scope;
    }
    match scope.index.as_deref() {
        Some(index) if opensearch_index(index) => {}
        _ => return ScopeFault::Scope,
    }
    if scope.observed == Some(true) {
        ScopeFault::Ok
    } else {
        ScopeFault::Observed
    }
}

fn sql_grant(name: &str) -> NativeGrant {
    if !known(PERMISSIONS, name) {
        NativeGrant::Unknown
    } else if known(READ_ONLY_PERMISSIONS, name) {
        NativeGrant::ReadOnly
    } else {
        NativeGrant::NotReadOnly
    }
}

fn mongo_grant(name: &str) -> NativeGrant {
    if name == "read" {
        NativeGrant::ReadOnly
    } else if known(MONGO_NOT_READONLY, name) {
        NativeGrant::NotReadOnly
    } else {
        NativeGrant::Unknown
    }
}

fn clickhouse_grant(name: &str) -> NativeGrant {
    if name == "SELECT" {
        NativeGrant::ReadOnly
    } else if name == "SHOW DATABASES" {
        NativeGrant::Companion
    } else if known(CLICKHOUSE_NOT_READONLY, name) {
        NativeGrant::NotReadOnly
    } else {
        NativeGrant::Unknown
    }
}

fn opensearch_grant(name: &str) -> NativeGrant {
    if name == OPENSEARCH_SEARCH || name == OPENSEARCH_MGET {
        NativeGrant::ReadOnly
    } else if opensearch_denied(name) {
        NativeGrant::NotReadOnly
    } else {
        NativeGrant::Unknown
    }
}

fn opensearch_denied(name: &str) -> bool {
    name.contains('*')
        || name.starts_with("indices:admin/")
        || name.starts_with("indices:data/write/")
        || name.starts_with("cluster:admin/")
        || known(OPENSEARCH_NOT_READONLY, name)
}

fn canonical_seconds(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_digit()
        && first != '0'
        && value.len() <= 9
        && chars.all(|ch| ch.is_ascii_digit())
}

fn clickhouse_ident(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 63
        && (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

pub(crate) fn opensearch_index(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 255
        && value != "_all"
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_sets_do_not_contain_product_tokens_or_read() {
        assert!(!known(COMPAT_MODES, MONGODB_NATIVE));
        assert!(!known(COMPAT_MODES, CLICKHOUSE_NATIVE));
        assert!(!known(COMPAT_MODES, STARROCKS_NATIVE));
        assert!(!known(COMPAT_MODES, DORIS));
        assert!(!known(COMPAT_MODES, OPENSEARCH_NATIVE));
        assert!(!known(PERMISSIONS, "read"));
        assert!(!known(READ_ONLY_PERMISSIONS, "read"));
        assert!(!known(READ_ONLY_PERMISSIONS, "readonly"));
        assert!(!PERMISSIONS.iter().any(|name| name.contains("read")));
        assert!(!known(CLICKHOUSE_NOT_READONLY, "SELECT"));
        assert!(!known(CLICKHOUSE_NOT_READONLY, "readonly"));
        assert!(!known(CLICKHOUSE_NOT_READONLY, "SHOW DATABASES"));
    }

    #[test]
    fn grants_are_exact_pairs() {
        assert_eq!(
            interpret_native_grant(MONGODB_NATIVE, "read"),
            NativeGrant::ReadOnly
        );
        assert_eq!(
            interpret_native_grant(MONGODB_NATIVE, "readWrite"),
            NativeGrant::NotReadOnly
        );
        assert_eq!(
            interpret_native_grant(MONGODB_NATIVE, "readAnyDatabase"),
            NativeGrant::NotReadOnly
        );
        assert_eq!(
            interpret_native_grant(MONGODB_NATIVE, "root"),
            NativeGrant::NotReadOnly
        );
        assert_eq!(
            interpret_native_grant(MONGODB_NATIVE, "reader"),
            NativeGrant::Unknown
        );
        assert_eq!(
            interpret_native_grant(MONGODB_NATIVE, "Read"),
            NativeGrant::Unknown
        );
        assert_eq!(
            interpret_native_grant(MONGODB_NATIVE, "SELECT"),
            NativeGrant::Unknown
        );
        assert_eq!(
            interpret_native_grant("postgres-native", "read"),
            NativeGrant::Unknown
        );
        assert_eq!(
            interpret_native_grant("postgres-native", "SELECT"),
            NativeGrant::ReadOnly
        );
        assert_eq!(
            interpret_native_grant("postgres-native", "INSERT"),
            NativeGrant::NotReadOnly
        );
        assert_eq!(
            interpret_native_grant(STARROCKS_NATIVE, "SELECT"),
            NativeGrant::ReadOnly
        );
        assert_eq!(interpret_native_grant(DORIS, "read"), NativeGrant::Unknown);
        assert_eq!(
            interpret_native_grant(CLICKHOUSE_NATIVE, "SELECT"),
            NativeGrant::ReadOnly
        );
        assert_eq!(
            interpret_native_grant(CLICKHOUSE_NATIVE, "INSERT"),
            NativeGrant::NotReadOnly
        );
        assert_eq!(
            interpret_native_grant(CLICKHOUSE_NATIVE, "SHOW DATABASES"),
            NativeGrant::Companion
        );
        assert_eq!(
            interpret_native_grant(CLICKHOUSE_NATIVE, "readonly"),
            NativeGrant::Unknown
        );
        assert_eq!(
            interpret_native_grant(OPENSEARCH_NATIVE, OPENSEARCH_SEARCH),
            NativeGrant::ReadOnly
        );
        assert_eq!(
            interpret_native_grant(OPENSEARCH_NATIVE, OPENSEARCH_MGET),
            NativeGrant::ReadOnly
        );
        assert_eq!(
            interpret_native_grant(OPENSEARCH_NATIVE, "indices:data/read/search*"),
            NativeGrant::NotReadOnly
        );
        assert_eq!(
            interpret_native_grant(OPENSEARCH_NATIVE, "indices:admin/create"),
            NativeGrant::NotReadOnly
        );
        assert_eq!(
            interpret_native_grant(OPENSEARCH_NATIVE, "indices:data/write/index"),
            NativeGrant::NotReadOnly
        );
        assert_eq!(
            interpret_native_grant(OPENSEARCH_NATIVE, "indices:data/read/get"),
            NativeGrant::Unknown
        );
        assert_eq!(
            interpret_native_grant(OPENSEARCH_NATIVE, "read"),
            NativeGrant::Unknown
        );
        assert_eq!(
            interpret_native_grant("tidb", "SELECT"),
            NativeGrant::ReadOnly
        );
        assert_eq!(
            interpret_native_grant("elasticsearch", "read"),
            NativeGrant::Unknown
        );
        assert!(!known(PERMISSIONS, "CREATE SESSION"));
        assert!(!known(PERMISSIONS, "FLASHBACK"));
        assert!(!known(READ_ONLY_PERMISSIONS, "EXECUTE"));
        assert_eq!(
            interpret_native_grant("postgres-native", "CREATE SESSION"),
            NativeGrant::Unknown
        );
        assert_eq!(
            interpret_native_grant("postgres-native", "EXECUTE"),
            NativeGrant::NotReadOnly
        );
        assert_eq!(
            interpret_native_grant("oracle-mode", "FLASHBACK"),
            NativeGrant::Unknown
        );
        assert_eq!(
            interpret_native_grant("oracle-mode", "CREATE SESSION"),
            NativeGrant::Unknown
        );
        assert_eq!(
            interpret_native_grant(ORACLE, "CREATE SESSION"),
            NativeGrant::ReadOnly
        );
        assert_eq!(
            interpret_native_grant(ORACLE, "FLASHBACK"),
            NativeGrant::ReadOnly
        );
        assert_eq!(
            interpret_native_grant(ORACLE, "EXECUTE"),
            NativeGrant::NotReadOnly
        );
        assert_eq!(
            interpret_native_grant(ORACLE, "FLASHBACK ANY TABLE"),
            NativeGrant::NotReadOnly
        );
        assert_eq!(
            interpret_native_grant(ORACLE, "SELECT ANY TABLE"),
            NativeGrant::NotReadOnly
        );
    }

    #[test]
    fn scopes_reject_wildcards_and_cross_engine_fields() {
        let database = GrantScope {
            database: Some("oo_dl01w40".to_string()),
            ..GrantScope::default()
        };
        let table = GrantScope {
            database: Some("system".to_string()),
            table: Some("columns".to_string()),
            ..GrantScope::default()
        };
        let star = GrantScope {
            database: Some("*".to_string()),
            ..GrantScope::default()
        };
        assert_eq!(clickhouse_select_fault(Some(&database)), ScopeFault::Ok);
        assert_eq!(clickhouse_select_fault(Some(&table)), ScopeFault::Ok);
        assert_eq!(clickhouse_select_fault(None), ScopeFault::Scope);
        assert_eq!(clickhouse_select_fault(Some(&star)), ScopeFault::Scope);
        let index = GrantScope {
            index: Some("oo-dl01w1-44-synthetic".to_string()),
            observed: Some(true),
            ..GrantScope::default()
        };
        let listed = GrantScope {
            index: Some("oo-dl01w1-44-synthetic".to_string()),
            observed: Some(false),
            ..GrantScope::default()
        };
        let wild = GrantScope {
            index: Some("oo-*".to_string()),
            observed: Some(true),
            ..GrantScope::default()
        };
        assert_eq!(opensearch_read_fault(Some(&index)), ScopeFault::Ok);
        assert_eq!(opensearch_read_fault(Some(&listed)), ScopeFault::Observed);
        assert_eq!(opensearch_read_fault(Some(&wild)), ScopeFault::Scope);
        assert!(clickhouse_companion_ok(None));
    }
}
