//! Control API, Git-backed config, and audited apply.
//!
//! Plan, apply, secrets, and RBAC are not implemented here.
//! [`file_hosting`] is the local upload-session slice.

#![forbid(unsafe_code)]

mod file_hosting;

pub use file_hosting::{
    AdmissionContext, FileHost, FileHostError, GrantEvidence, HostLimits, IdempotencyKey,
    MAX_OBJECT_BYTES, MAX_PART_COUNT, ObjectKey, ObjectManifest, PartReceipt, PrincipalId,
    ProjectId, SessionId, StoredObject, UploadConfig, UploadSession, idempotency_key,
};
