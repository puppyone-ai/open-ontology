//! Run-stop facts. Detail text is a short static reason, never a password.

use std::io;

pub struct Stop {
    pub step: String,
    pub detail: String,
}

impl Stop {
    pub fn new(step: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            step: step.into(),
            detail: detail.into(),
        }
    }
}

pub fn disk(step: &str, err: &io::Error) -> Stop {
    Stop::new(step, err.kind().to_string())
}
