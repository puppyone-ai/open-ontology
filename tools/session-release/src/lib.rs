#![forbid(unsafe_code)]

//! Local-fixture bounded release orchestrator. Not a production sandbox.

mod backend;
mod checkpoint;
mod clock;
mod drive;
mod harness;
mod model;
mod paths;
mod release;
mod sha256;

pub use model::{ReleaseReceipt, ReleaseRequest, StepView, Unsupported, CONTRACT};
pub use release::{run, RunResult};

pub fn sha256_hex(input: &[u8]) -> Option<String> {
    sha256::sha256(input).map(|digest| sha256::hex(&digest))
}
