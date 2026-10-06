#![forbid(unsafe_code)]

mod audit;
mod check;
mod error;
mod json;
mod model;
mod sha256;

pub use error::Fault;
pub use sha256::sha256_hex;

#[derive(Debug)]
pub enum Outcome {
    Valid(model::Report),
    Invalid(model::Report),
}

impl Outcome {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid(_))
    }

    pub fn json(&self) -> Result<Vec<u8>, Fault> {
        let report = match self {
            Self::Valid(report) | Self::Invalid(report) => report,
        };
        serde_json::to_vec(report).map_err(|_| Fault::request())
    }
}

pub fn validate(bytes: &[u8]) -> Result<Outcome, Fault> {
    check::validate(bytes)
}
