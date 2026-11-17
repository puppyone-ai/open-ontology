//! Super Ontology document pipeline (spec `specs/super-ontology/10-backend.md`).
//!
//! Files in a Drive flow through ingest -> parse -> normalize (`oo.doc-ir/v1`)
//! -> extract -> resolve -> review -> publish -> compile (`oo.context-pack/v1`).
//! The pipeline only runs for explicitly registered Drives and the whole crate
//! is reachable from the CLI only behind the `docs` feature, so database users
//! are unaffected (30 §1).
//!
//! Module ownership follows 70 §2: each top-level module belongs to one board;
//! this root file, shared types, and contracts belong to the integration thread.

#![forbid(unsafe_code)]

/// Version of the engine binary.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Control API contract version served by the engine (10 §8).
pub const API_VERSION: &str = "docs-api/1.0";
/// Context Pack protocol (D25).
pub const CONTEXT_PACK_PROTOCOL: &str = "oo.context-pack/v1";
/// Engine configuration protocol (10 §2.8).
pub const CONFIG_PROTOCOL: &str = "oo.docs/v1";

pub mod api;
pub mod cas;
pub mod compile;
pub mod config;
pub mod extract;
pub mod health;
pub mod ingest;
pub mod jobs;
pub mod normalize;
pub mod obs;
pub mod parse;
pub mod publish;
pub mod resolve;
pub mod review;
pub mod share;
pub mod state;
pub mod store;
pub mod temporal;

#[cfg(feature = "failpoints")]
pub mod failpoints;
