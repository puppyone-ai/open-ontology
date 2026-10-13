//! Independent MP-01a–d acceptance judge.
//!
//! The judge reads a closed JSON bundle and the sealed typed spool. It does not
//! open a source connection. `run_producer` is the only process spawn, and it
//! returns before spawn unless the bundle pins a fixed single-connection binary.
//! A verdict from this crate is not an enterprise copy and not MP-02–06.

#![forbid(unsafe_code)]

mod case;
mod fail;
mod json;
mod judge;
mod run;
mod sha;
mod typed;

pub use case::{
    parse_case, AfterSeal, Audit, Budgets, Campaign, Case, Cell, Class, Conn, Fault, Fingerprint,
    FixtureClass, Producer, Provenance, Row, Snapshot, Table, Writes, CAMPAIGN_OWNER, FACT_GUARD,
    FACT_SINGLE, PRODUCER_OWNER, PROTOCOL,
};
pub use fail::{fail, Fail};
pub use judge::{judge, verdict_json, Card, Echo, Verdict};
pub use run::{run_producer, RunReport};
pub use sha::{hex_ok, sha256_hex};
pub use typed::{canonical_cell, canonical_preimage, cell_ok, reconcile_table, TableReport};
