//! Bounded HTTP client for the public gateway.
//!
//! `query` sends a [`QueryRequest`] and returns a [`QueryResponse`]. Knowledge,
//! describe, tables, validate, freshness, operations, status, and cancel call
//! the same origin. A server id (`s_`) uses `/v1/operations`. A query id (`q_`)
//! uses `/v1/queries`. `reqwest`'s JSON helper is not used. This crate does not open a
//! database, cache results, or read tokens from the environment or the filesystem.
//!
//! Automatic retry is only for a complete `RATE_LIMITED` or `OVERLOADED`
//! response to `query`, and only when the caller left that policy enabled.
//! Other calls are one attempt. Transport failures are not replayed. A client
//! timeout means this process stopped waiting. It is not a confirmation that
//! a database backend cancelled work.
//!
//! Export, Arrow, discover, resolve, search-values, sandbox environment
//! loading, and an OpenAPI client are not implemented.

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
    "resolve",
    "search_values",
    "from_sandbox_env",
    "openapi_client",
];
