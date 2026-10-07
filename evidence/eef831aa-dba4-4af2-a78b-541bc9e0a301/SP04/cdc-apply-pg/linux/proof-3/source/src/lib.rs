//! Fenced writer for one sealed normalized CDC transaction.
//! Not an acknowledgement, a control mirror, or a segmented loader.

#![forbid(unsafe_code)]

mod apply;
mod model;
mod tls;

pub use apply::apply;
pub use model::{
    binding_from, config_preimage, convert_cell, digest_text, hook_preimage, manifest_matches,
    payload_preimage, plan_tables, schema_preimage, type_preimage, Binding, ColumnDecl, Endpoint,
    KeyDecl, Output, PgType, Request, Segment, Stored, TablePlan, APPLY_LIMIT, CONTRACT,
};
pub use tls::commit_query_visible;
