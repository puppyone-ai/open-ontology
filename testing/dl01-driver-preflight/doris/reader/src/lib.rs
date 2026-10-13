//! Apache Doris native reader. mysql_async is the only wire client.
//! This crate does not grade the source and does not call a builder.

pub mod audit;
pub mod capture;
pub mod cell;
pub mod civil;
pub mod decimal;
pub mod dba;
pub mod fail;
pub mod gate;
pub mod guard;
pub mod ipc;
pub mod session;
pub mod sqltext;
