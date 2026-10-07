//! Inventory and expiry for objects a `FileHost` has already issued.
//!
//! An expired completed object is deleted by [`oo_control::FileHost::delete_completed`].
//! This crate does not unlink store files and does not edit quota rows.

#![forbid(unsafe_code)]

mod deploy;
mod inventory;
mod permit;
mod reaper;
mod text;

pub use deploy::Deployment;
pub use inventory::{RegisterOpen, RegisterPublished, Registration, RegistrationKind};
pub use permit::{PermitDecision, enforce_download_ttl};
pub use reaper::{ObjectOutcome, PauseAfter, ReapReport, report_json};

use std::time::{SystemTime, UNIX_EPOCH};

pub const CONTRACT: &str = "oo.artifact-retention/v1";

pub const FILEHOST_CONTRACT: &str = "oo.control.local-file-hosting/v1";

pub const UPLOADED_SOURCE_TTL_CAP_SECONDS: u64 = 180 * 24 * 60 * 60;
pub const SESSION_OUTPUT_TTL_CAP_SECONDS: u64 = 30 * 24 * 60 * 60;
pub const WORKSPACE_SNAPSHOT_TTL_CAP_SECONDS: u64 = 24 * 60 * 60;
pub const TRANSCRIPT_TTL_CAP_SECONDS: u64 = 180 * 24 * 60 * 60;
pub const QUARANTINE_TTL_CAP_SECONDS: u64 = 7 * 24 * 60 * 60;
pub const EVAL_TTL_CAP_SECONDS: u64 = 365 * 24 * 60 * 60;
pub const DOWNLOAD_PERMIT_TTL_CAP_SECONDS: u64 = 5 * 60;

/// Host and gateway work this crate does not pretend to finish.
///
/// Completed-object delete and its one quota release are implemented.
/// The names below are still open.
pub fn required_integration() -> &'static [&'static str] {
    &[
        "filehost_part_absence_observation",
        "filehost_dest_staging_reap",
        "filehost_object_version",
        "filehost_admission_role_and_policy",
        "gateway_authenticated_url_revocation",
        "formula_safe_csv_excel",
        "sessions_outputs_prefix",
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetentionClass {
    UploadedSource,
    SessionOutput,
    WorkspaceSnapshot,
    Transcript,
    Quarantine,
    Eval,
}

impl RetentionClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UploadedSource => "uploaded_source",
            Self::SessionOutput => "session_output",
            Self::WorkspaceSnapshot => "workspace_snapshot",
            Self::Transcript => "transcript",
            Self::Quarantine => "quarantine",
            Self::Eval => "eval",
        }
    }

    pub fn cap_seconds(self) -> u64 {
        match self {
            Self::UploadedSource => UPLOADED_SOURCE_TTL_CAP_SECONDS,
            Self::SessionOutput => SESSION_OUTPUT_TTL_CAP_SECONDS,
            Self::WorkspaceSnapshot => WORKSPACE_SNAPSHOT_TTL_CAP_SECONDS,
            Self::Transcript => TRANSCRIPT_TTL_CAP_SECONDS,
            Self::Quarantine => QUARANTINE_TTL_CAP_SECONDS,
            Self::Eval => EVAL_TTL_CAP_SECONDS,
        }
    }

    fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "uploaded_source" => Ok(Self::UploadedSource),
            "session_output" => Ok(Self::SessionOutput),
            "workspace_snapshot" => Ok(Self::WorkspaceSnapshot),
            "transcript" => Ok(Self::Transcript),
            "quarantine" => Ok(Self::Quarantine),
            "eval" => Ok(Self::Eval),
            _ => Err(Error::Corrupt),
        }
    }
}

/// `Production` reads the process clock. `Explicit` is a test instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Now {
    Explicit(u64),
    Production,
}

pub fn resolve_now(now: Now) -> Result<u64, Error> {
    match now {
        Now::Explicit(millis) => Ok(millis),
        Now::Production => {
            let dur = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::Clock)?;
            u64::try_from(dur.as_millis()).map_err(|_| Error::Clock)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Admission,
    NotOwnedRoot,
    Symlink,
    ForeignPath,
    GuessedKey,
    NotFound,
    Denied,
    Conflict,
    Quota,
    TtlCap,
    WorkspaceDisabled,
    ClassMismatch,
    TupleMismatch,
    Expired,
    Drift,
    Corrupt,
    Store,
    ReadErrorNotAbsence,
    Unsupported { seam: &'static str },
    Clock,
    Limit,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Admission => "admission value is not usable",
            Self::NotOwnedRoot => "deployment root is not the owner directory",
            Self::Symlink => "symlink is refused",
            Self::ForeignPath => "path is outside the owner deployment",
            Self::GuessedKey => "object key was not issued by this host",
            Self::NotFound => "registration is not in the inventory",
            Self::Denied => "admission context does not match this object",
            Self::Conflict => "registration conflicts with the stored row",
            Self::Quota => "project quota would be exceeded",
            Self::TtlCap => "ttl is outside the frozen cap",
            Self::WorkspaceDisabled => "workspace retention is off unless explicitly enabled",
            Self::ClassMismatch => "retention class does not match the request",
            Self::TupleMismatch => "current role or policy does not match the registration",
            Self::Expired => "registration is past its expiry",
            Self::Drift => "stored object does not match the registration",
            Self::Corrupt => "durable record is not usable",
            Self::Store => "object store operation failed",
            Self::ReadErrorNotAbsence => "read failed and absence was not established",
            Self::Unsupported { .. } => "required host api is not available",
            Self::Clock => "clock is not usable",
            Self::Limit => "audit record limit was reached",
        })
    }
}

impl std::error::Error for Error {}

pub(crate) fn expiry_millis(created_unix_ms: u64, ttl_seconds: u64) -> Result<u64, Error> {
    let span = ttl_seconds.checked_mul(1000).ok_or(Error::TtlCap)?;
    created_unix_ms.checked_add(span).ok_or(Error::TtlCap)
}

pub(crate) fn check_ttl(
    class: RetentionClass,
    ttl_seconds: u64,
    explicit_workspace: bool,
) -> Result<(), Error> {
    if explicit_workspace && class != RetentionClass::WorkspaceSnapshot {
        return Err(Error::ClassMismatch);
    }
    if class == RetentionClass::WorkspaceSnapshot && !explicit_workspace {
        return Err(Error::WorkspaceDisabled);
    }
    if ttl_seconds == 0 || ttl_seconds > class.cap_seconds() {
        return Err(Error::TtlCap);
    }
    Ok(())
}
