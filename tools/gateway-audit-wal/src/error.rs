use std::fmt;
use std::io;

#[derive(Debug)]
pub enum WalError {
    Busy,
    Quota,
    Corrupt,
    Conflict,
    Bound,
    Io(io::Error),
    Tls,
    Protocol,
    AckMismatch,
    Timeout,
}

impl WalError {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::Quota => "quota",
            Self::Corrupt => "corrupt",
            Self::Conflict => "conflict",
            Self::Bound => "bound",
            Self::Io(_) => "io",
            Self::Tls => "tls",
            Self::Protocol => "protocol",
            Self::AckMismatch => "ack_mismatch",
            Self::Timeout => "timeout",
        }
    }
}

impl fmt::Display for WalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl std::error::Error for WalError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    Sync,
    Write,
    NoSpace,
    Quota,
    Corrupt,
}

impl FaultKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Write => "write",
            Self::NoSpace => "no_space",
            Self::Quota => "quota",
            Self::Corrupt => "corrupt",
        }
    }
}

pub(crate) fn io_fault(err: &io::Error) -> FaultKind {
    if err.raw_os_error() == Some(28) || err.kind() == io::ErrorKind::StorageFull {
        FaultKind::NoSpace
    } else if err.kind() == io::ErrorKind::Other && err.to_string().contains("injected sync") {
        FaultKind::Sync
    } else {
        FaultKind::Write
    }
}
