//! Read-only data gateway.
//!
//! SP06-W01 installs scoped column ACLs for a trusted administrator.
//! Authentication, SQL admission, audit, and row-level security are not implemented here.

#![forbid(unsafe_code)]

pub mod policy;

pub use policy::{
    compile_column_acl, connect_require_tls, install_column_acl, rustls_connector, ClassStatus,
    ColumnAclPolicy, ColumnSpec, CompiledPolicy, InstallEffect, InstallOutcome, LogicalTable,
    PiiClass, PolicyError, RowFilter, TlsConnection, TlsEndpoint,
};
