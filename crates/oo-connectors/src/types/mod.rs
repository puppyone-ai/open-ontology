//! PostgreSQL lossless conversion, protocol `oo.source.pgsql.types/v1`.
//!
//! This is not the frozen `oo.source.pgsql/v1` catalog. Catalog
//! `type_support.converted` stays false and `numeric_parts` stays the
//! unsigned inspection decoder. A caller flag cannot mark a column converted.
//! Values are decoded from the PostgreSQL binary wire payload. Text output
//! functions are not used.

mod arrow_batch;
mod binary;
mod batch;
mod facts;
mod mapping;
mod read;
mod typmod;

#[cfg(test)]
mod tests;

pub use batch::{
    assemble_batch, arrow_buffer_preimage, ArrowColumn, ArrowScalar, CellPresence, CellReject,
    DecodedCell, MaskedReject, TypedBatch, TypedRow,
};
pub use facts::{TypeFacts, TypeGraph};
pub use mapping::{map_column, ArrowPhysical, ColumnMapping, ColumnPlan, MapDecision};
pub use read::{compose_select, valid_quoted_ident};
pub use typmod::{numeric_typmod_v1, NumericTypmodV1};

// Crate-visible bridge so the snapshot owner can call the approved decoder
// and plan guard. Mapping and decode semantics stay the ones in `binary` and
// `read`. This is not a catalog or access collect API.
pub(crate) use binary::decode_payload;
pub(crate) use read::{
    compose_key_scan, compose_presence_scan, compose_scan, guard_plans, require_binary,
};

use crate::model::CapabilityStatus;

/// Conversion protocol. Independent of `oo.source.pgsql/v1`.
pub const TYPE_CONVERSION_PROTOCOL: &str = "oo.source.pgsql.types/v1";

/// Signed numeric typmod rule used by this protocol.
pub const NUMERIC_TYPMOD_VERSION: &str = "pg-numeric-typmod/v1";

/// What to do with a column that has no lossless plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsupportedColumnPolicy {
    /// Omit the column from the read. The cell is unavailable and the row stays.
    /// A key column still fails the read.
    PreserveRow,
    /// Fail the read before any value of that column is fetched.
    FailRead,
}

/// How a decoded cell that cannot be represented is recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectCellPolicy {
    /// Keep the row. The cell is unavailable, not SQL NULL, and not a coerced value.
    PreserveRow,
}

/// Bound for one typed read. The digest is of the catalog, config, and policy
/// inputs the caller claims. This type does not recompute that digest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunBinding {
    pub source_id: String,
    pub schema: String,
    pub table: String,
    pub run_generation: u64,
    pub input_digest: [u8; 32],
}

/// Policy for one read. There is no mode that drops a row or rewrites a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PgTypePolicy {
    pub unsupported: UnsupportedColumnPolicy,
    pub reject: RejectCellPolicy,
}

impl PgTypePolicy {
    pub const PRESERVE_ROW: Self = Self {
        unsupported: UnsupportedColumnPolicy::PreserveRow,
        reject: RejectCellPolicy::PreserveRow,
    };
}

impl Default for PgTypePolicy {
    fn default() -> Self {
        Self::PRESERVE_ROW
    }
}

/// Capabilities of this conversion protocol. The frozen catalog list is unchanged.
pub fn type_capabilities() -> Vec<CapabilityStatus> {
    [
        ("pg_builtin_lossless", true, "binary wire values for the enterprise builtins"),
        ("pg_signed_numeric_typmod", true, NUMERIC_TYPMOD_VERSION),
        ("pg_domain_base", true, "domain values decode as the base builtin"),
        ("pg_enum_label", true, "enum binary label, checked against catalog labels"),
        ("pg_array_lower_bound", true, "lower bounds are part of the canonical struct"),
        ("pg_arrow_series60", true, "workspace ArrowTypeName spellings"),
        (
            "pg_arrow_crate",
            true,
            "arrow-array, arrow-buffer, and arrow-schema 60.0.0 build a RecordBatch",
        ),
        ("mysql_unsigned", false, "not this slice"),
        ("position_codec", false, "not this slice"),
        ("cdc", false, "not this slice"),
        ("bounded_reader", false, "not this slice"),
        ("loader_reject_persistence", false, "reject records are returned, not stored"),
        ("user_defined_output", false, "custom send functions are not selected"),
    ]
    .into_iter()
    .map(|(name, supported, reason)| CapabilityStatus {
        name: name.to_string(),
        supported,
        reason: reason.to_string(),
    })
    .collect()
}

pub(crate) fn digest_hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
