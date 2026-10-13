//! Shared contracts for Open Ontology.
//!
//! This crate is types and pure rules: configuration, Ontology Spec fragments,
//! batch metadata, API envelopes, and the canonical logical-value hash.
//! It does not open a database, speak HTTP, load a driver, resolve a secret,
//! or decode a source protocol. SP00-W02 owns the models and the strict
//! parser. SP00-W03 owns canonical bytes. A full HTTP SDK is SP09.

#![forbid(unsafe_code)]

pub mod api;
pub mod arrow_meta;
pub mod batch;
pub mod canonical;
pub mod config;
pub mod convert;
pub mod decimal;
pub mod diagnostic;
pub mod duration;
pub mod json_scan;
pub mod jsonb_doc;
pub mod limits;
pub mod oos;
pub mod pattern_map;
pub mod rfc3339;
pub mod schema_export;
pub mod schema_util;
pub mod sealed_import;
pub mod source_bound;
pub mod text;
pub mod tz;
pub mod yaml;

pub use decimal::{DecimalError, DecimalStorage, ExactDecimal, ExactInt, FlexDecimal, Position};
pub use diagnostic::{
    API_VERSION, BATCH_PROTOCOL_VERSION, CANONICAL_PK_VERSION, CANONICAL_VALUE_VERSION,
    CONFIG_SCHEMA_VERSION, CONTRACT_API, CONTRACT_BATCH, CONTRACT_CANON, CONTRACT_CONFIG,
    CONTRACT_OOS, Diagnostic, DiagnosticList, ERROR_SCHEMA_VERSION, OOS_SCHEMA_VERSION, Severity,
    TYPE_POLICY_V1,
};
pub use duration::{ByteSize, Duration};
pub use json_scan::parse_json_value;
pub use yaml::{parse_yaml_typed, parse_yaml_value, scan_yaml};
