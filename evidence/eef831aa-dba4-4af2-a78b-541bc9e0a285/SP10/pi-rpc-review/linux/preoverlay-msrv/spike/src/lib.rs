//! Bounded Pi RPC stdio driver for the pinned upstream build.
//! Stub tests in this crate do not approve the upstream protocol.

#![forbid(unsafe_code)]

mod drive;
mod json;
mod map;

pub use drive::{encode_command, Command, Process, Spawn, Stop};
pub use json::{parse_value, Value};
pub use map::{map_record, MapResult, ProductEvent, Sequencer, Sequenced};

use std::fmt;

pub const DEFAULT_LINE_LIMIT: usize = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Shape,
    Io,
    Deadline,
    Eof,
    Partial,
    TooBig,
    Spawn,
    Protocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    pub detail: String,
}

impl Error {
    pub fn new(kind: Kind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = self.kind;
        let detail = &self.detail;
        write!(f, "{kind:?}: {detail}")
    }
}

impl std::error::Error for Error {}
