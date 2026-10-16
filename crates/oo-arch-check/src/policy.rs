//! Frozen workspace membership and the crate classes the gate rejects.
//!
//! Gateway may later use `tokio-postgres` for the read-only Agent DB. It may not
//! use SQLx: that driver is the control-plane and source-connector stack.
//!
//! These name checks are a reviewed taxonomy of known package names. A match is
//! not a proof of what an arbitrary crate does, and a name missing from the
//! taxonomy is not approval to link it. Every new dependency still needs
//! supply-chain and capability review.

pub const R_CYCLE: &str = "R-CYCLE";
pub const R_DEP_KIND_UNKNOWN: &str = "R-DEP-KIND-UNKNOWN";
pub const R_DEP_UNRESOLVED: &str = "R-DEP-UNRESOLVED";
pub const R_EDGE_MISSING: &str = "R-EDGE-MISSING";
pub const R_EDGE_UNDECLARED: &str = "R-EDGE-UNDECLARED";
pub const R_MEMBER_DUPLICATE: &str = "R-MEMBER-DUPLICATE";
pub const R_MEMBER_MISSING: &str = "R-MEMBER-MISSING";
pub const R_MEMBER_UNKNOWN: &str = "R-MEMBER-UNKNOWN";
pub const R_MEMBER_UNRESOLVED: &str = "R-MEMBER-UNRESOLVED";
pub const R_RESOLVE_DANGLING: &str = "R-RESOLVE-DANGLING";
pub const R_RESOLVE_MISSING: &str = "R-RESOLVE-MISSING";

pub const R_CORE_DB: &str = "R-CORE-DB";
pub const R_CORE_MODEL: &str = "R-CORE-MODEL";
pub const R_CORE_SANDBOX: &str = "R-CORE-SANDBOX";
pub const R_CORE_WORKSPACE_DEP: &str = "R-CORE-WORKSPACE-DEP";

pub const R_GATEWAY_ADMIN_DDL: &str = "R-GATEWAY-ADMIN-DDL";
pub const R_GATEWAY_CONNECTOR: &str = "R-GATEWAY-CONNECTOR";
pub const R_GATEWAY_PIPELINE: &str = "R-GATEWAY-PIPELINE";
pub const R_GATEWAY_SOURCE_CREDENTIAL: &str = "R-GATEWAY-SOURCE-CREDENTIAL";
pub const R_GATEWAY_RUNTIME: &str = "R-GATEWAY-RUNTIME";
pub const R_GATEWAY_SOURCE_DRIVER: &str = "R-GATEWAY-SOURCE-DRIVER";

pub const R_CLIENT_DB_DRIVER: &str = "R-CLIENT-DB-DRIVER";
pub const R_ANALYSIS_DB_DRIVER: &str = "R-ANALYSIS-DB-DRIVER";
pub const R_TOOL_FORBIDDEN: &str = "R-TOOL-FORBIDDEN";

/// Product crates plus this gate. A new workspace member is an architecture change.
pub const REQUIRED_MEMBERS: &[&str] = &[
    "oo-analysis",
    "oo-arch-check",
    "oo-builder",
    "oo-cli",
    "oo-connectors",
    "oo-control",
    "oo-core",
    "oo-entry",
    "oo-eval",
    "oo-gateway",
    "oo-jobs",
    "oo-mcp",
    "oo-pipeline",
    "oo-proxy",
    "oo-sandbox",
    "oo-sdk",
    "xtask",
];

/// Exact direct workspace edges. Adding or removing one updates this table.
pub const REQUIRED_EDGES: &[(&str, &str)] = &[
    ("oo-analysis", "oo-core"),
    ("oo-builder", "oo-core"),
    ("oo-builder", "oo-jobs"),
    ("oo-cli", "oo-core"),
    ("oo-cli", "oo-sdk"),
    ("oo-connectors", "oo-core"),
    ("oo-control", "oo-core"),
    ("oo-control", "oo-jobs"),
    ("oo-entry", "oo-core"),
    ("oo-eval", "oo-core"),
    ("oo-gateway", "oo-builder"),
    ("oo-gateway", "oo-core"),
    ("oo-jobs", "oo-core"),
    ("oo-mcp", "oo-core"),
    ("oo-pipeline", "oo-connectors"),
    ("oo-pipeline", "oo-core"),
    ("oo-pipeline", "oo-jobs"),
    ("oo-proxy", "oo-core"),
    ("oo-sandbox", "oo-core"),
    ("oo-sdk", "oo-core"),
    ("xtask", "oo-arch-check"),
    ("xtask", "oo-core"),
];

pub fn is_required_edge(from: &str, to: &str) -> bool {
    REQUIRED_EDGES
        .iter()
        .any(|(edge_from, edge_to)| *edge_from == from && *edge_to == to)
}

pub fn is_source_credential_crate(name: &str) -> bool {
    name == "oo-connectors"
}

pub fn is_admin_ddl_crate(name: &str) -> bool {
    name == "oo-control"
}

pub fn is_sqlx_family(name: &str) -> bool {
    name == "sqlx" || name.starts_with("sqlx-") || name.starts_with("sqlx_")
}

pub fn is_mysql_driver(name: &str) -> bool {
    matches!(
        name,
        "mysql" | "mysql_async" | "mysql_common" | "mysqlclient-sys" | "mysqlclient-src"
    ) || name.starts_with("mysql_async-")
}

pub fn is_diesel_family(name: &str) -> bool {
    name == "diesel" || name.starts_with("diesel-") || name.starts_with("diesel_")
}

pub fn is_sea_orm_family(name: &str) -> bool {
    name == "sea-orm" || name.starts_with("sea-orm-") || name.starts_with("sea-orm_")
}

pub fn is_libsql_family(name: &str) -> bool {
    name == "libsql" || name.starts_with("libsql-") || name.starts_with("libsql_")
}

pub fn is_duckdb_driver(name: &str) -> bool {
    name == "duckdb" || name == "libduckdb-sys" || name.starts_with("duckdb-")
}

pub fn is_sqlite_driver(name: &str) -> bool {
    name == "rusqlite" || name == "libsqlite3-sys"
}

pub fn is_sync_postgres_driver(name: &str) -> bool {
    name == "postgres"
}

/// Read-only Agent DB stack reserved for Gateway and the loader.
pub fn is_gateway_postgres_stack(name: &str) -> bool {
    matches!(
        name,
        "tokio-postgres" | "postgres-types" | "postgres-protocol" | "deadpool-postgres"
    )
}

pub fn is_redis_client(name: &str) -> bool {
    name == "redis" || name == "fred"
}

pub fn is_sandbox_orchestrator(name: &str) -> bool {
    matches!(name, "bollard" | "kube" | "k8s-openapi")
}

pub fn is_model_sdk(name: &str) -> bool {
    matches!(name, "async-openai" | "anthropic" | "rig-core")
        || name == "openai"
        || name.starts_with("openai-")
        || name.starts_with("openai_")
        || name.starts_with("async-openai-")
        || name.starts_with("async-openai_")
}

/// Drivers and ORMs that must not appear on oo-core, data clients, analysis, or the gate.
///
/// Serde, schema crates, and local Arrow or file crates (`arrow`, `arrow-schema`,
/// `parquet`) are not in this set. `sea-orm-arrow` is, because that name is the
/// SeaORM family rather than a local Arrow type.
pub fn is_client_database_driver(name: &str) -> bool {
    is_sqlx_family(name)
        || is_mysql_driver(name)
        || is_duckdb_driver(name)
        || is_sqlite_driver(name)
        || is_sync_postgres_driver(name)
        || is_gateway_postgres_stack(name)
        || is_redis_client(name)
        || is_diesel_family(name)
        || is_sea_orm_family(name)
        || is_libsql_family(name)
}

/// Source and control drivers. Gateway's own tokio-postgres stack is not in this set.
pub fn is_gateway_forbidden_driver(name: &str) -> bool {
    is_sqlx_family(name)
        || is_mysql_driver(name)
        || is_duckdb_driver(name)
        || is_sqlite_driver(name)
        || is_sync_postgres_driver(name)
        || is_diesel_family(name)
        || is_sea_orm_family(name)
        || is_libsql_family(name)
}
