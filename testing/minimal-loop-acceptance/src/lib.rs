//! External acceptance card for the single-operator development loop.
//!
//! The card measures a fixed producer archive and calls that archive's binary.
//! It does not accept the parent plan, runsc, or arbitrary databases.

#![forbid(unsafe_code)]

mod card;
mod fixture;
mod gold;
mod json;
mod judge;
mod sha256;

pub use card::{measure_kit, run_card, CardOpts};
pub use fixture::write_fixture;
pub use gold::Cell;
pub use json::{parse, Value};
pub use sha256::sha256_hex;
pub use judge::{
    config_is_secret_free, fresh_copy_performed, judge, FaultObs, MeasuredFile, Observation, QuestionObs, ToolObs,
    Verdict,
};
