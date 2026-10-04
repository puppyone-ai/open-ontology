//! Structure-draft compiler.
//!
//! A bounded `oo.builder.catalog-evidence/v1` envelope becomes a reviewable
//! `oo.builder.structure-draft/v1` document. `compile_captured_pg_catalog`
//! builds that envelope from one captured `oo.source.pgsql/v1` catalog.
//! `compile_role_pack` turns one effective OOS document and one visibility
//! snapshot into a bounded role pack. This crate does not connect to a
//! database, call a model, grant privileges, certify entities, or publish.

#![forbid(unsafe_code)]

mod canonical;
mod captured_pg;
mod draft;
mod error;
mod ident;
mod limits;
mod nfc;
mod pgsql;
mod role_pack;

pub use captured_pg::{
    CAPTURED_COMPILER, CAPTURED_PROTOCOL, CapturedPgDraft, compile_captured_pg_catalog,
};
pub use role_pack::{RolePack, RolePackError, compile_role_pack, normalize_pack_path};
pub use draft::{COMPILER, DRAFT_PROTOCOL, compile_structure_draft};
pub use error::{DraftError, EVIDENCE_PROTOCOL};
pub use nfc::nfc;
pub use pgsql::EMPTY_SCHEMA_HASH;
