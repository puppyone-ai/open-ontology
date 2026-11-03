//! Optional Oracle source support, exported by the `oracle` feature.
//! `oo-oracle-source` and the focused preflight harness share this module.
pub mod arrow_part;
pub mod chunk;
pub mod cli;
pub mod keyset;
pub mod rules;
mod server_metadata;
mod temporal;
mod timestamp;
