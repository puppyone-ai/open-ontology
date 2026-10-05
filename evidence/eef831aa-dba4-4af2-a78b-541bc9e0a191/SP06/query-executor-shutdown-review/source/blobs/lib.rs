//! Read-only data gateway.
//!
//! SP06-W01 installs scoped column ACLs for a trusted administrator.
//! SP06-W02 admits one `SELECT` through the PostgreSQL parser and a caller-supplied
//! policy snapshot. `query_executor` runs that admitted statement on one dedicated
//! session for a trusted runtime. Authentication, durable audit, and row-level
//! security are not implemented here.

#![forbid(unsafe_code)]

pub mod policy;
pub mod query_executor;
pub mod sql_guard;

pub use policy::{
    compile_column_acl, connect_require_tls, install_column_acl, rustls_connector, ClassStatus,
    ColumnAclPolicy, ColumnSpec, CompiledPolicy, InstallEffect, InstallOutcome, LogicalTable,
    PiiClass, PolicyError, RowFilter, TlsConnection, TlsEndpoint,
};
pub use sql_guard::{
    guard_select, BoundFunction, BoundOperator, BoundOutputColumn, BoundRelation, GuardError,
    GuardedPlan, LiteralKind, LiteralTypes, OutputItem, PolicyCatalog, PolicyIdentity,
    RecognizedBindings, ResolvedFunction, ResolvedOperator, StarExpansion, VisibleColumn,
    VisibleRelation, DEPTH_BUDGET, NODE_BUDGET, PINNED_PARSER_VERSION, SQL_BYTE_BUDGET,
};
