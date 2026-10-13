//! Read-only data gateway.
//!
//! SP06-W01 installs scoped column ACLs for a trusted administrator.
//! SP06-W02 admits one `SELECT` through the PostgreSQL parser and a caller-supplied
//! policy snapshot. `query_executor` runs that admitted statement on one dedicated
//! session for a trusted runtime. `parser_worker` runs the guard in a one-shot
//! child process and kills that child at 500 ms. Kernel network, filesystem, and
//! memory isolation are not provided, so untrusted admission stays blocked.
//! `http_gateway` serves loopback HTTP and TLS from the accepted opaque token store.
//! `public_surface` serves the authenticated query, knowledge, describe, tables,
//! validate, freshness, and cancel routes. Its audit record is a local fsync
//! WAL. Row filters stay refused. Export and MCP are not served.

#![forbid(unsafe_code)]

include!(concat!(env!("OUT_DIR"), "/accepted_glue.rs"));

pub mod http_error;
pub mod http_gateway;
pub mod mapping_reader_access;
pub mod pack_catalog;
pub mod parser_worker;
pub mod policy;
pub mod public_surface;
pub mod query_executor;
pub mod reviewed_policy;
pub mod sql_guard;

pub use policy::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, CompiledPolicy, InstallEffect, InstallOutcome,
    LogicalTable, PiiClass, PolicyError, RowFilter, TlsConnection, TlsEndpoint, compile_column_acl,
    connect_require_tls, install_column_acl, rustls_connector,
};
pub use reviewed_policy::{
    ReviewedAllow, ReviewedBinding, ReviewedColumnPolicies, ReviewedIdentity, ReviewedPolicyError,
    ReviewedSourceSeal, compile_reviewed_policies,
};
pub use sql_guard::{
    BoundFunction, BoundOperator, BoundOutputColumn, BoundRelation, DEPTH_BUDGET, GuardError,
    GuardedPlan, LiteralKind, LiteralTypes, NODE_BUDGET, OutputItem, PINNED_PARSER_VERSION,
    PolicyCatalog, PolicyIdentity, RecognizedBindings, ResolvedFunction, ResolvedOperator,
    SQL_BYTE_BUDGET, StarExpansion, VisibleColumn, VisibleRelation, guard_select,
};
