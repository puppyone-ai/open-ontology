#![forbid(unsafe_code)]

mod canonical;
mod produce;
mod scan;
mod score;
mod sha256;

pub use canonical::{canonical_json, canonical_sha256};
pub use produce::{produce, seal_card};
pub use score::score;

pub const MAX_INPUT_BYTES: usize = 1_048_576;
pub const CATALOG_COMMIT: &str = "149944271ef1c2cb555954d65248413df2166f1c";
pub const CARDS_COMMIT: &str = "c23db28dc27f5cf20741dd36e8a8e88129ad1715";
pub const REVIEW_PROTOCOL: &str = "oo.selfbuild-rules.review-cards/v1";
pub const TRUTH_PROTOCOL: &str = "oo.selfbuild-rules.truth/v1";
pub const SCORE_PROTOCOL: &str = "oo.selfbuild-rules.score/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fail {
    pub code: &'static str,
}

impl Fail {
    pub const fn new(code: &'static str) -> Self {
        Self { code }
    }
}
