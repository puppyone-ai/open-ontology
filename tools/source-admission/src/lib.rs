//! Source admission library. Policy admission and source audit are separate.

#![forbid(unsafe_code)]

mod admit;
mod audit;
mod budget_adapt;
mod capture;
#[path = "../../../crates/oo-pipeline/src/native_c5_range.rs"]
mod native_c5_range;
#[path = "../../../crates/oo-pipeline/src/native_c5_parts.rs"]
mod native_c5_parts;
#[path = "../../../crates/oo-pipeline/src/native_c5_multipart.rs"]
pub mod native_c5_multipart;
mod checklist;
mod model;
mod native_map;
mod mysql_full;
mod observe;
mod parse;
mod pg_adapt;
mod recipe;

pub use admit::admit;
pub use mysql_full::{mysql_full_audit_valid, mysql_full_grant_audit_valid};
pub use audit::classify_csv;
pub use budget_adapt::admit_pg_budget;
pub use capture::{
    CapturedInput, CapturedInputV2, InputFault, OutputPaths, PartSource, SealBundle, SealResult,
    WITNESS_PROTOCOL_V2, WitnessFiles, WitnessFilesV2, decide_captured, decide_captured_v2,
    seal_captured, seal_captured_v2, witness_is_v2, witness_paths, witness_paths_v2,
};
pub use checklist::redact;
pub use model::*;
pub use native_map::{
    NativeGrant, PRODUCT_COMPAT_MODES, compat_recognized, engine_for_compat,
    interpret_native_grant, seal_engine,
};
pub use observe::{ObserveError, observe};
pub use parse::{ParseError, parse_strict};
pub use pg_adapt::adapt;
pub use recipe::{
    OPEN_SERVER_REGISTRATION_MAX_BYTES, REGISTRATION_RECIPE_PROTOCOL, RegistrationRecipe,
    consumer_published, quota_for, recipe_json, registration_recipe,
};

pub fn decision_json(decision: &Decision) -> Result<Vec<u8>, ParseError> {
    let mut bytes = serde_json::to_vec_pretty(decision).map_err(|err| ParseError {
        fact: err.to_string(),
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}
mod oracle_buffer;
