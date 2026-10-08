//! Static failure tokens. Feedback text is never a token.

use std::io;

#[derive(Debug)]
pub enum RecvError {
    Bound,
    Tls,
    Protocol,
    Forbidden,
    CrossProject,
    Conflict,
    Oversized,
    Producer,
    Db,
    Io(io::Error),
}

impl RecvError {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Bound => "bound",
            Self::Tls => "tls",
            Self::Protocol => "protocol",
            Self::Forbidden => "forbidden",
            Self::CrossProject => "cross_project",
            Self::Conflict => "conflict",
            Self::Oversized => "oversized",
            Self::Producer => "producer",
            Self::Db => "db",
            Self::Io(_) => "io",
        }
    }

    pub fn status(&self) -> (u16, &'static str) {
        match self {
            Self::Forbidden | Self::CrossProject => (403, "Forbidden"),
            Self::Conflict => (409, "Conflict"),
            Self::Oversized | Self::Protocol | Self::Bound => (400, "Bad Request"),
            Self::Producer | Self::Tls | Self::Db | Self::Io(_) => (500, "Unavailable"),
        }
    }
}

impl std::fmt::Display for RecvError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name())
    }
}
