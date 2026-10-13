use std::fmt;

/// Operator-facing failure. The display line is a closed token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fail {
    pub token: &'static str,
    pub exit: u8,
}

impl Fail {
    pub const fn new(token: &'static str, exit: u8) -> Self {
        Self { token, exit }
    }
}

impl fmt::Display for Fail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "postgres-preview: {}", self.token)
    }
}

impl std::error::Error for Fail {}
