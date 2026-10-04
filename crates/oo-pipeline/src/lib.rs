//! Sync pipeline and loader.
//!
//! SP00-W01 reserves this crate. CDC and snapshot reads are not implemented here.
//! `target_installer` installs the frozen target schema on a caller-owned connection.
//! It is not the Loader and it does not admit a source reader.
//! `physical_mapping` binds a `pgident/v1` logical name to a stored PostgreSQL identifier.
//! It does not write user data, shadows, or receipts, and it is not the Loader.
//! `copy_encoder` emits bounded COPY binary rows and is not the Loader.
//! `copy_sink` sends those bytes on the caller's connection.
//! `full_import` is the unpublished full-import entry. It is not a publication.
//! `import_cli` is the `import-pg` command. It calls that entry and does not
//! install the target schema.
//! `catalog_cli` is the `catalog-pg` command. It does not install a target
//! schema or import rows.

#![forbid(unsafe_code)]

pub mod catalog_cli;
pub mod copy_encoder;
pub mod copy_sink;
pub mod full_import;
pub mod import_cli;
pub mod physical_mapping;
pub mod target_installer;
