use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fail {
    pub code: &'static str,
    pub fact: String,
}

impl Fail {
    pub fn new(code: &'static str, fact: impl Into<String>) -> Self {
        Self {
            code,
            fact: fact.into(),
        }
    }
}

impl fmt::Display for Fail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.fact)
    }
}

impl std::error::Error for Fail {}
