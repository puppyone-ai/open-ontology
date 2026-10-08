//! Orchestrates the ontology main chain by calling producers.
//!
//! The producers pinned in `assess` do not yet expose the YAML export, the
//! reviewed-tree commit, the Git gate evaluation, or the active-pack binding.
//! This crate records that gap and spawns nothing. It does not write YAML,
//! decisions, commits, gate files, or packs.

mod assess;
mod http;
mod sha256;

pub use assess::{assess_root, assess_texts, claims_success, to_json, Assessment, Gap, CONTRACT, SOURCE_HEAD};
pub use http::{bind_loopback, serve_once};
pub use sha256::{hex, sha256};
