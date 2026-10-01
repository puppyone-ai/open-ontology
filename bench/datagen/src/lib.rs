//! Standalone OO-Logistics xs/s generator.
//!
//! This crate is its own Cargo workspace. It is not a root workspace member
//! and it is not a product driver. A later HARNESS change is the only place
//! that should add it to the root membership list.

pub mod attacks;
pub mod authz;
pub mod catalog;
pub mod cli;
pub mod csvfile;
pub mod ddl;
pub mod digest;
pub mod encoding;
pub mod error;
pub mod export;
pub mod facts;
pub mod generate;
pub mod jsonutil;
pub mod pathsafety;
pub mod plan;
pub mod prng;
pub mod rows;
pub mod truth;
pub mod uscc;
pub mod value;
pub mod verify;

pub use error::{Error, Result};
pub use export::export_sources;
pub use generate::{Options, Report, generate};
pub use verify::verify_dataset;
