//! Local fsync spool for the existing `DedicatedSession` audit hook.
//!
//! `DiskWal` implements [`AuditAdmission`]. It does not construct `Admission`
//! or `CompletionRecord`, and it does not execute SQL. `wal-send` posts sealed
//! frames to the audit collector. A checkpoint is written only after that
//! collector's HTTPS acknowledgement matches the frame.

#![forbid(unsafe_code)]

mod deliver;
mod error;
mod frame;
mod store;

pub use deliver::{deliver_sealed, load_send_config, match_ack, post_event_body, DeliveryStop, SendConfig, VerifiedAck};
pub use error::{FaultKind, WalError};
pub use oo_gateway::query_executor::AuditAdmission;
pub use store::{inspect, DiskWal, FrameView, Inspect, RequestView, SealedFrame, WalConfig};
