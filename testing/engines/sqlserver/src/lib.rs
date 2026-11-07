//! Compile the lane-owned adapter without modifying the coordinator-owned export.
pub use oo_connectors::{SourceError, engine};
#[path = "../../../../crates/oo-connectors/src/sqlserver/mod.rs"]
pub mod sqlserver;
