//! Native PostgreSQL 17.10 controlled preview orchestration.
//!
//! This crate does not link the live workspace `oo-pipeline`, `oo-builder`,
//! `oo-connectors`, or `oo-core` trees. Admission, import, description, and
//! preview binaries are configured paths. A missing W38 fixed commit is a
//! pending dependency, not an admitted source.

#![forbid(unsafe_code)]

mod admit;
mod arrow_out;
mod audit_local;
mod bind;
mod capture;
mod chain;
mod cli;
mod consume;
mod config;
mod error;
mod logistics;
mod oracle;
mod pg;
mod plan;
mod receipt;
mod spawn;

pub use admit::{
    budget_block_is_missing_producer, decision_admitted, read_w38_commit, session_report,
    AdmissionInput, SessionFacts,
};
pub use arrow_out::{write_logistics_batch, ArrowRow};
pub use audit_local::classify_csv;
pub use cli::cli_main;
pub use config::{load_config, Config};
pub use error::Fail;
pub use logistics::{
    copy_event, copy_hub, copy_shipment, cost_text, hub_token, EVENT_ROWS, EXACT_COST, HUB_ROWS,
    SHIPMENT_ROWS,
};
pub use oracle::{oracle_statements, OracleCheck};
pub use pg::{
    cancel_sleep, connect, deadline_sleep, load_logistics, observe_session, prepare_target_schema, read_password,
    Endpoint, Session,
};
pub use plan::{model_cache_key, model_http, plan_run, same_run, Plan, PlanInput};
pub use spawn::{argv_has_secret, run_binary};
