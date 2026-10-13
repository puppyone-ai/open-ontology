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
//! `catalog_import` is the `catalog-import` command. It reads one sealed
//! MySQL import from the target and does not open a source or install a schema.
//! `catalog-import-pg` reads one sealed PostgreSQL import. It does not retag
//! that engine as MySQL and it does not open a source.
//! `catalog-import-native` reads one sealed native import. The request has no
//! engine field. MySQL and PostgreSQL stay on their own commands.
//! `target_statistics` is an explicit operator `ANALYZE` of mapped shadows.
//! It is not part of import and it does not publish.
//! `native_arrow` imports a typed Arrow batch onto PostgreSQL storage.
//! A semantic catalog is not a physical batch.
//! `native_update` applies one source generation of upserts and tombstones
//! onto a new unpublished shadow. It is not a second full snapshot and it
//! does not publish.
//! `jsonb_document` imports one sealed BSON document as jsonb oid 3802.
//! The column type comes from `_oo.full_import_type_sql` in `0009`.
//! Binary COPY writes `jsonb_send`. This path does not `ALTER TYPE`, does not
//! import a PostgreSQL source, and does not publish.

#![forbid(unsafe_code)]

pub mod catalog_cli;
pub mod catalog_import;
pub mod jsonb_document;
pub mod native_catalog;
pub mod copy_encoder;
pub mod copy_sink;
pub mod full_import;
pub use full_import::{approved_column_typmod, approved_columns, ApprovedColumn};
pub mod import_cli;
mod native_binding;
pub mod native_arrow;
pub mod native_update;
pub mod physical_mapping;
pub mod target_installer;
pub mod target_statistics;
