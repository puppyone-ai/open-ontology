//! Budgets for documents, batches, and canonical values.
//!
//! These are pure structural limits. They are not runtime admission control.

/// Caller-enforced YAML/JSON document size. `serde-saphyr` only applies
/// `max_reader_input_bytes` to readers, so `from_str` is checked here first.
pub const DOCUMENT_MAX_BYTES: usize = 1_048_576;
pub const YAML_MAX_DEPTH: usize = 32;
pub const YAML_MAX_NODES: usize = 50_000;
pub const YAML_MAX_ALIASES: usize = 0;
pub const YAML_MAX_ANCHORS: usize = 0;
pub const YAML_MAX_DOCUMENTS: usize = 1;
pub const YAML_MAX_INCLUSION_DEPTH: u32 = 0;
pub const YAML_MAX_EVENTS: usize = 200_000;
pub const YAML_MAX_SCALAR_BYTES: usize = DOCUMENT_MAX_BYTES;
pub const YAML_MAX_COMMENT_BYTES: usize = 65_536;

pub const JSON_MAX_DEPTH: usize = 32;
pub const JSON_MAX_NODES: usize = 50_000;

pub const BATCH_MAX_ROWS: usize = 10_000;
pub const BATCH_MAX_BYTES: usize = 16 * 1024 * 1024;

pub const CANONICAL_MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
pub const CANONICAL_MAX_VALUE_BYTES: usize = 1_048_576;
pub const CANONICAL_MAX_FIELD_NAME_BYTES: usize = 256;
pub const CANONICAL_MAX_COEFFICIENT_DIGITS: usize = 8_192;
pub const CANONICAL_MAX_ARRAY_ELEMENTS: usize = 100_000;
pub const CANONICAL_MAX_DEPTH: usize = 32;

pub const QUERY_SQL_MAX_BYTES: usize = 64 * 1024;
pub const INSTRUCTIONS_MAX_BYTES: usize = 16 * 1024;
/// Query cell text cap from REQ-GW-05. This one is Unicode scalars.
pub const QUERY_CELL_MAX_CHARS: usize = 500;
pub const ERROR_TEXT_MAX_CHARS: usize = 2_000;
pub const ROW_FILTER_MAX_CHARS: usize = 2_000;
pub const DURATION_MAX_SECONDS: u64 = 366 * 10 * 24 * 60 * 60;

/// MySQL `TIME` bounds, in microseconds: `-838:59:59.000000 ..= 838:59:59.000000`.
pub const MYSQL_TIME_MAX_MICROS: i64 = (838 * 3600 + 59 * 60 + 59) * 1_000_000;

/// Inclusive timestamptz second bounds used by the canonical payload (UTC).
pub const TIMESTAMPTZ_MIN_SECONDS: i64 = -62_135_596_800;
pub const TIMESTAMPTZ_MAX_SECONDS: i64 = 253_402_300_799;

pub const DECIMAL128_MAX_PRECISION: usize = 38;
pub const DECIMAL256_MAX_PRECISION: usize = 76;
