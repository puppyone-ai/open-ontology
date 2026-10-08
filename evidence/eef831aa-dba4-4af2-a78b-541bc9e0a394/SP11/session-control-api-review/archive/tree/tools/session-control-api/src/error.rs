//! API faults. The reason is a stable code, never a password or a driver string.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    Malformed(&'static str),
    Unauthenticated,
    Forbidden(&'static str),
    NotFound,
    Conflict(&'static str),
    Incomplete(&'static str),
    Unavailable(&'static str),
}

impl Fault {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Malformed(reason) | Self::Forbidden(reason) | Self::Conflict(reason) | Self::Incomplete(reason) | Self::Unavailable(reason) => reason,
            Self::Unauthenticated => "unauthenticated",
            Self::NotFound => "not_found",
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::Malformed(_) => 422,
            Self::Unauthenticated => 401,
            Self::Forbidden(_) => 403,
            Self::NotFound => 404,
            Self::Conflict(_) | Self::Incomplete(_) => 409,
            Self::Unavailable(_) => 503,
        }
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.reason())
    }
}
