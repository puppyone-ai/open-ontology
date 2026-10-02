//! Ontology Spec v1 documents.
//!
//! `ontology.yaml` carries `spec_version`. Every other fragment in this crate
//! requires `schema_version: oos/v1` because a single file cannot inherit the
//! manifest version. Narrative samples omit that field; they are not valid
//! fragments until it is added. Anchor expressions are checked, not executed.

mod merge;
mod model;
mod parse;

pub use merge::{FactPin, MergeOutcome, RemovedItem, StaleOverride, merge_effective};
pub use model::*;
pub use parse::*;
