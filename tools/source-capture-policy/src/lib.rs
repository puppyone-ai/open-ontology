//! Capture planner. It returns operator decisions from declared state and
//! from real `cdc-spool` / `cdc-offset-store` receipt functions.
//! It does not start a scheduler, write a loader generation, or mirror a
//! control database.

#![forbid(unsafe_code)]

mod gate;
mod model;
mod plan;
mod receipts;

pub use model::{
    plan_bytes, AsOfInput, CaptureMethod, CursorInput, CursorSafety, Decision, DeleteDetection,
    Grade, JoinInput, MethodInput, OffsetBinding, OffsetCheckpointRef, OffsetDeclared, OffsetInput,
    OffsetNext, OffsetReceipt, OffsetWatermark, PlanRequest, Profile, RequestedConsistency,
    RetentionInput, SupportGrade, SyncMode, TableInput, TokenOrder, WriteClass, CONTRACT,
};
pub use plan::plan_request;
