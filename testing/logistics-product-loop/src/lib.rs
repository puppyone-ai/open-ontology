//! Logistics product-loop fixture, independent gold, and acceptance card.
//!
//! This crate does not accept the parent plan, production runsc, or an arbitrary
//! database. A draft preview is not S9 approval or S10 publication.

#![forbid(unsafe_code)]

mod emit;
mod fixture;
mod gold;
mod json;
mod judge;
mod sha256;

pub use emit::{canonical_hash, schema_sql, write_fixture, write_gold};
pub use fixture::{
    badge, canonical_event as canonical_event_line, canonical_hub as canonical_hub_line, canonical_shipment as canonical_shipment_line,
    capacity_grams, cost_coeff, cost_text, dest_hub, event_hub, event_shipment, format_scale4, opened_at, origin_hub, shipment_status, transport_mode,
    unix_micros, EVENT_ROWS, EXACT_COST, HUB_ROWS, SHIPMENT_ROWS,
};
pub use gold::{
    answers_absent_from_questions, cells, delayed_air_count, delivered_origin_cost, null_cost_count, prompt_leaks, questions,
};
pub use judge::{judge, verdict_text, Verdict};
pub use json::{parse, stringify, Value};
pub use sha256::sha256_hex;
