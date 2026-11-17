//! Token counting for packs: `oo.token-estimate/v2` (10 §7.5), the single
//! estimator every budget, manifest and INDEX figure uses.

/// Estimator name written into manifests and parameters.
pub const ESTIMATOR: &str = oo_builder::TOKEN_ESTIMATOR_V2;

/// Tokens of `text`.
pub fn count(text: &str) -> u64 {
    oo_builder::cjk_calibrated_tokens(text)
}
