//! Sync pipeline and loader.
//!
//! SP00-W01 reserves this crate. CDC, snapshot, and target writes are SP04 and are not implemented here.
//! `physical_mapping` binds a `pgident/v1` logical name to a stored PostgreSQL identifier.
//! It does not write user data, shadows, or receipts, and it is not the Loader.
//! `copy_encoder` emits bounded COPY binary rows and is not the Loader.

#![forbid(unsafe_code)]

pub mod copy_encoder;
pub mod physical_mapping;
