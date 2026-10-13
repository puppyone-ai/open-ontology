//! Structure-draft compiler.
//!
//! A bounded `oo.builder.catalog-evidence/v1` envelope becomes a reviewable
//! `oo.builder.structure-draft/v1` document. `compile_captured_pg_catalog`
//! builds that envelope from one captured `oo.source.pgsql/v1` catalog.
//! `compile_reviewed_effective` turns one sealed reviewed captured draft,
//! explicit classification decisions, and explicit ontology settings into
//! `oo.builder.effective-oos/v1`. It does not invent a PII level or a
//! semantic class. `compile_role_pack` turns one effective OOS document and
//! one visibility snapshot into a bounded role pack. `compile_role_pack_command`
//! is the stdin entry for that function. The visibility value is
//! caller-supplied public data, not a live permission proof. This crate does
//! not connect to a database, call a model, grant privileges, certify
//! entities, or publish.

#![forbid(unsafe_code)]

mod canonical;
mod captured_pg;
mod description_candidate;
mod draft;
mod error;
mod ident;
mod limits;
mod nfc;
mod pgsql;
mod reviewed_effective;
mod role_pack;
mod role_pack_cli;
mod jsonb_profile;
mod sealed_import;

pub use captured_pg::{
    CAPTURED_COMPILER, CAPTURED_PROTOCOL, CapturedPgDraft, compile_captured_pg_catalog,
};
pub use description_candidate::{
    DESCRIPTION_CANDIDATE_COMPILER, DESCRIPTION_CANDIDATE_PROTOCOL, DESCRIPTION_PROMPT_VERSION,
    DESCRIPTION_REQUEST_COMMAND_PROTOCOL, DESCRIPTION_REQUEST_PROTOCOL,
    DESCRIPTION_RESPONSE_PROTOCOL, DescriptionCandidateError, complete_description_candidates,
    prepare_description_request,
};
pub use reviewed_effective::{
    REVIEWED_EFFECTIVE_COMMAND_PROTOCOL, REVIEWED_EFFECTIVE_COMPILER, REVIEWED_EFFECTIVE_PROTOCOL,
    ReviewedEffectiveError, compile_reviewed_effective, compile_reviewed_sealed_import,
};
pub use role_pack::{RolePack, RolePackError, compile_role_pack, normalize_pack_path};
pub use jsonb_profile::{
    JSONB_PROFILE_COMPILER, JSONB_PROFILE_PROTOCOL, JsonbDocumentProfile,
    compile_jsonb_document_profile,
};
pub use sealed_import::{
    REVIEWED_SEALED_IMPORT_COMMAND_PROTOCOL, REVIEWED_SEALED_IMPORT_COMPILER,
    REVIEWED_SEALED_IMPORT_PROTOCOL, SEALED_IMPORT_COMPILER, SealedImportColumnDraft,
    SealedImportDraft, SealedImportError, SealedImportTableDraft, compile_sealed_import_catalog,
    compile_sealed_import_collection, SEALED_IMPORT_COLLECTION_PROTOCOL,
    identity_preimage,
};
pub use role_pack_cli::{RolePackCommandError, compile_role_pack_command};
pub use draft::{COMPILER, DRAFT_PROTOCOL, compile_structure_draft};
pub use error::{DraftError, EVIDENCE_PROTOCOL};
pub use nfc::nfc;
pub use pgsql::EMPTY_SCHEMA_HASH;
