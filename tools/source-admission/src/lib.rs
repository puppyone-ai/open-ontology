//! Source admission library. Policy admission and source audit are separate.

#![forbid(unsafe_code)]

mod admit;
mod audit;
mod budget_adapt;
mod capture;
mod checklist;
mod model;
mod native_map;
mod observe;
mod parse;
mod pg_adapt;
mod recipe;

pub use admit::admit;
pub use audit::classify_csv;
pub use budget_adapt::admit_pg_budget;
pub use capture::{
    CapturedInput, InputFault, OutputPaths, SealBundle, SealResult, WitnessFiles, decide_captured,
    seal_captured, witness_paths,
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
