//! OpenSearch batch encoding for one preflight. This is not `oo-core` and it
//! does not call a SourceCatalog API.

#![allow(clippy::uninlined_format_args, clippy::redundant_closure_for_method_calls)]

pub mod bind;
pub mod c5;
pub mod cast;
pub mod catalog;
pub mod ipc;

pub const PROTOCOL: &str = "oo.source.opensearch.batch/v1";
