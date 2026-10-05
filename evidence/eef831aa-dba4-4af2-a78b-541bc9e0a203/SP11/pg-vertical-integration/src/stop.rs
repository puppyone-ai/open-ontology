//! Integration stop record. A stop is an observed API seam, not a product patch.

use std::fmt;

pub const FIXED_BASE: &str = "0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1";

#[derive(Clone, Debug)]
pub struct Stop {
    pub step: &'static str,
    pub input: String,
    pub expected: String,
    pub observed: String,
    pub owned_path_needed: String,
}

impl Stop {
    pub fn new(
        step: &'static str,
        input: impl Into<String>,
        expected: impl Into<String>,
        observed: impl Into<String>,
        owned_path_needed: impl Into<String>,
    ) -> Self {
        Self {
            step,
            input: input.into(),
            expected: expected.into(),
            observed: observed.into(),
            owned_path_needed: owned_path_needed.into(),
        }
    }
}

impl fmt::Display for Stop {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{step}: expected {expected}; observed {observed}",
            step = self.step,
            expected = self.expected,
            observed = self.observed,
        )
    }
}
