//! Structure-draft compiler.
//!
//! A bounded `oo.builder.catalog-evidence/v1` envelope becomes a reviewable
//! `oo.builder.structure-draft/v1` document. This crate does not connect to a
//! database, call a model, grant privileges, certify entities, or publish.

#![forbid(unsafe_code)]

mod canonical;
mod draft;
mod error;
mod ident;
mod limits;
mod nfc;
mod pgsql;

pub use draft::{COMPILER, DRAFT_PROTOCOL, compile_structure_draft};
pub use error::{DraftError, EVIDENCE_PROTOCOL};
pub use nfc::nfc;
pub use pgsql::EMPTY_SCHEMA_HASH;
