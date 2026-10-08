//! Authenticated clients for the ontology main chain.
//!
//! A producer is called only when its success schema requires that edge's
//! fields. An operationId line, a hash match, or HTTP 409 does not close the
//! chain. This crate spawns no child process and writes no YAML, commit, gate
//! file, or pack.

mod assess;
mod client;
mod discover;
mod execute;
mod http;
mod journal;
mod sha256;

pub use assess::{assess_root, assess_texts, claims_success, to_json, Assessment, Gap, CONTRACT, SOURCE_HEAD};
pub use client::{session_ok, Session};
pub use execute::{chain_proven, execute, EdgeProof, LoopProof, Plan};
pub use http::{bind_loopback, serve_once};
pub use journal::cleanup;
pub use sha256::{hex, sha256};
