//! Static failures. Messages do not echo paths, hashes, or Git output.

use std::fmt;

pub const PROTOCOL: &str = "oo.curated-git-apply/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratedGitError {
    pub code: &'static str,
    pub message: &'static str,
    pub compiler_code: Option<String>,
}

impl CuratedGitError {
    pub const fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            compiler_code: None,
        }
    }

    pub fn refused(compiler_code: String) -> Self {
        Self {
            code: "review.refused",
            message: "the description review compiler refused the command",
            compiler_code: Some(compiler_code),
        }
    }
}

impl fmt::Display for CuratedGitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{PROTOCOL}/{} {}", self.code, self.message)
    }
}

impl std::error::Error for CuratedGitError {}
