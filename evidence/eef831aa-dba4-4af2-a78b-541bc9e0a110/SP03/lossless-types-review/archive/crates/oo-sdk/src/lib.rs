//! Bounded HTTP client for `POST /v1/query`.
//!
//! The client sends a [`QueryRequest`] and returns a [`QueryResponse`] or a
//! sanitized error. It checks the frozen `oo-core` JSON door before the
//! request is sent and again on the response. `reqwest`'s JSON helper is not
//! used. This crate does not open a database, cache query results, or read
//! tokens from the environment or the filesystem.
//!
//! Automatic retry is only for a complete `RATE_LIMITED` or `OVERLOADED`
//! gateway envelope, and only when the caller left that policy enabled.
//! Transport failures are not replayed. A client timeout means this process
//! stopped waiting. It is not a confirmation that a database backend cancelled
//! work.
//!
//! Export, Arrow, discover, describe, resolve, search, freshness, sandbox
//! environment loading, and query cancellation are not implemented.

#![forbid(unsafe_code)]

mod client;
mod config;
mod endpoint;
mod error;
mod token;
mod wire;

pub use client::Client;
pub use config::{ByteCaps, ClientConfig, RetryPolicy, RootTrust, Timeouts, TransportMode};
pub use error::{GatewayError, LocalError, ProtocolError, QueryError, TransportError};
pub use oo_core::api::{ErrorCode, QueryRequest, QueryResponse};
pub use token::BearerToken;

/// Public query body ceiling from REQ-GW-03. Callers may set a lower cap.
pub const QUERY_HTTP_MAX_BYTES: usize = oo_core::limits::DOCUMENT_MAX_BYTES;

/// Public `max_rows` ceiling. A response with more rows is rejected.
pub const QUERY_MAX_ROWS: usize = 2_000;

/// Capabilities this slice does not implement.
pub const UNSUPPORTED: &[&str] = &[
    "export",
    "arrow",
    "discover",
    "describe",
    "resolve",
    "search_values",
    "freshness",
    "from_sandbox_env",
    "query_cancellation",
    "openapi_client",
];
