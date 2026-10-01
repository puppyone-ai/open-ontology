//! Budgets for one structure-draft compilation.
//!
//! These bounds are part of `oo.builder.catalog-evidence/v1`. A larger catalog
//! is refused. This slice does not sample rows and does not read a database.

/// Tables in one evidence envelope.
pub const MAX_TABLES: usize = 64;
/// Columns on one table.
pub const MAX_COLUMNS: usize = 256;
/// Primary, unique, and foreign keys on one table, plus the selection key list.
pub const MAX_KEYS: usize = 32;
/// Columns in one key or foreign key.
pub const MAX_KEY_COLUMNS: usize = 16;
/// Row-security policies on one table.
pub const MAX_POLICIES: usize = 32;
/// ACL entries accepted and then discarded. They are not part of the draft.
pub const MAX_PRIVILEGES: usize = 64;
/// Enum labels recorded only as a hash.
pub const MAX_ENUM_LABELS: usize = 64;
/// Domain-check hashes on one column.
pub const MAX_DOMAIN_CHECKS: usize = 16;
/// Review decisions in one envelope.
pub const MAX_REVIEWS: usize = 1024;
/// Curated dataset or relationship overlays.
pub const MAX_CURATED_ITEMS: usize = 64;
/// PostgreSQL identifier byte ceiling, including a quoted identifier.
pub const MAX_NAME_BYTES: usize = 63;
/// Claimed UTF-8 length of an untrusted comment or expression.
pub const MAX_UNTRUSTED_BYTES: u64 = 65_536;
/// Review item id byte ceiling.
pub const MAX_ITEM_ID_BYTES: usize = 512;
