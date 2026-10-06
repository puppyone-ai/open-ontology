//! Explicit review of description candidates into curated dataset text.
//!
//! `apply_description_review` re-executes `complete_description_candidates`
//! on the original command string. An approval may write `label`,
//! `description`, and `grain` onto an existing `DatasetFile`. The library
//! does not open a database, write a file, call a model, grant, certify,
//! authenticate, or publish.

#![forbid(unsafe_code)]

mod apply;
mod canonical;
mod error;

pub use apply::{apply_description_review, curated_dataset_sha256, descriptive_field_sha256};
pub use error::{COMMAND_PROTOCOL, REVIEW_COMPILER, REVIEW_PROTOCOL, DescriptionReviewError};
