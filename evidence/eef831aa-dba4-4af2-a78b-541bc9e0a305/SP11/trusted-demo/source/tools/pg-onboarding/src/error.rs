use std::fmt;

#[derive(Debug)]
pub struct Stop {
    pub step: String,
    pub detail: &'static str,
}

impl Stop {
    pub fn new(step: impl Into<String>, detail: &'static str) -> Self {
        Self {
            step: step.into(),
            detail,
        }
    }
}

impl fmt::Display for Stop {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "pg-onboarding: {} {}", self.step, self.detail)
    }
}
