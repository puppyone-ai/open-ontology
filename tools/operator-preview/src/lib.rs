//! Trusted local operator preview.
//!
//! This crate binds one operator file to one sealed target, one role policy,
//! and one immutable draft pack. It does not open a public HTTP route, mint a
//! role, or launch Pi. Untrusted admission stays the gateway constant.

#![forbid(unsafe_code)]

mod authority;
mod cancel_run;
mod describe;
mod durable;
mod native_query;
mod native_types;
mod native_wal;
mod output;
mod query_run;

pub use authority::{load_authority, load_policy, load_query};
pub use cancel_run::cancel;
pub use describe::describe;
pub use output::Finished;
pub use query_run::query;

pub const CONFIG_PROTOCOL: &str = "oo.operator-preview.config/v1";
pub const POLICY_PROTOCOL: &str = "oo.operator-preview.policy/v1";
pub const QUERY_PROTOCOL: &str = "oo.operator-preview.query/v1";
pub const RESULT_PROTOCOL: &str = "oo.operator-preview.result/v1";
pub const DESCRIBE_PROTOCOL: &str = "oo.operator-preview.describe/v1";
pub const CANCEL_PROTOCOL: &str = "oo.operator-preview.cancel/v1";
pub const RECEIPT_PROTOCOL: &str = "oo.operator-preview.receipt/v1";
pub const BINDS: &str = "import+mapping+role-policy+pack+target-tls";
pub const SURFACE: &str = "trusted-local-operator-preview";
pub const CANDIDATE_PROTOCOL: &str = "oo.minimal-product-loop.draft-candidate/v1";
pub const PACK_RECEIPT_PROTOCOL: &str = "oo.minimal-product-loop.pack-receipt/v1";

pub const EXIT_OK: u8 = 0;
pub const EXIT_IO: u8 = 1;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_REQUEST: u8 = 3;
pub const EXIT_TLS: u8 = 4;
pub const EXIT_POLICY: u8 = 5;
pub const EXIT_GUARD: u8 = 6;
pub const EXIT_AUDIT: u8 = 7;
pub const EXIT_QUERY: u8 = 8;
pub const EXIT_PACK: u8 = 9;
pub const EXIT_NOT_SENT: u8 = 10;
