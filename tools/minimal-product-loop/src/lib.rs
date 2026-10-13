//! Operator development loop.
//!
//! One local operator. Draft knowledge is not a publication. The gateway path
//! shells the existing `query-pg` binary, which calls `DedicatedSession`.
//! This crate does not implement a SELECT executor.

#![forbid(unsafe_code)]

mod ask;
mod bind;
mod close;
mod config;
mod copy_invoke;
mod doctor;
mod forward;
mod gateway;
mod proxy;
mod gold;
mod local_catalog;
mod pack;
mod pack_input;
mod run;
mod source;
mod state;

pub use ask::{ask, prompt_for, AskOutcome};
pub use close::{cancel, close, CancelOutcome, CloseOutcome};
pub use config::{load_config, Config, ConfigError, OPERATOR_PI_GID, OPERATOR_PI_UID};
pub use pack_input::{sealed_pack_input, PACK_INPUT_PROTOCOL, PACK_RECEIPT_PROTOCOL};
pub use doctor::{doctor, ready_bins, report_json, DoctorReport};
pub use gateway::{map_stdout, QueryPgOutcome};
pub use gold::{bulk_questions, expected_cells, frozen_questions, question_text_for, FrozenQuestion};
pub use pack::write_draft;
pub use run::{prepare, run, RunOutcome};
pub use copy_invoke::invoke as copy;
pub use source::CONNECTOR_PROTOCOL;

pub const PROTOCOL: &str = "oo.minimal-product-loop/v1";
pub const DEVELOPMENT_ACK: &str = "local-process-not-a-sandbox-certificate";
