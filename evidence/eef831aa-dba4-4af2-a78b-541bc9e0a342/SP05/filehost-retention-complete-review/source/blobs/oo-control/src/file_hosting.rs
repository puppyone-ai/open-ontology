//! Local multipart upload sessions on an [`object_store::ObjectStore`].
//!
//! The caller passes an [`AdmissionContext`] that a future control admission step
//! has already accepted. This module does not authenticate that context, does not
//! speak HTTP, and does not treat an unguessable key as authorization.
//!
//! Object keys and session ids are generated here. Callers never supply a
//! filesystem path for an upload, part, or read. [`FileHost::open_download`]
//! pulls a published object in bounded ranges. [`FileHost::delete_completed`]
//! removes one completed object under the deployment lock. That object's quota
//! reservation stays until its bytes, sidecars, parts, and platform staging are
//! absent, and only then is the reservation released once. [`FileHost::open_local`]
//! is the operator entry that binds a deployment directory to [`object_store::local::LocalFileSystem`].
//! [`FileHost::new`] accepts any store so a later approved backend can use the
//! same calls. A store without the local deployment lock cannot enforce a
//! linearizable quota, and mutating calls on that host fail before they write.
//! This crate does not enable the S3 feature and does not claim that the local
//! store is S3.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::Waker;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use object_store::path::Path;
use object_store::{
    GetOptions, GetRange, GetResult, MultipartUpload, ObjectStore, ObjectStoreExt, PutMode,
    PutMultipartOptions, PutOptions, PutPayload,
};
use sha2::{Digest, Sha256};

const CONTRACT: &str = "oo.control.local-file-hosting/v1";
const IDEM_DOMAIN: &[u8] = b"oo.file-hosting/v1 idempotency\0";
const LINEAGE_DOMAIN: &[u8] = b"oo.file-hosting/v1 lineage\0";
const MAX_IDENTITY_BYTES: usize = 128;
const MAX_GRANT_BYTES: usize = 512;
const MAX_DISPLAY_BYTES: usize = 255;
const MAX_CONTENT_TYPE_BYTES: usize = 128;

/// Absolute ceiling from REQ-SRC-05. A host limit cannot raise it.
pub const MAX_OBJECT_BYTES: u64 = 5_000_000_000;
/// S3 allows 10_000 parts. This slice uses the same count ceiling.
pub const MAX_PART_COUNT: u32 = 10_000;
const DEFAULT_PROJECT_BYTES: u64 = 50_000_000_000;
/// Upper bound configured before each ranged read and each multipart write of a stored part.
///
/// Caller bytes passed to `put_part`, and the buffer `read_object` returns, are outside this bound.
/// `open_download` refuses a chunk budget above it and never builds a larger range.
const STREAM_CHUNK_BYTES: usize = 8 * 1024;
/// A mutating call fails closed if the deployment lock is still busy after this long.
const LOCK_WAIT: Duration = Duration::from_secs(30);
const LOCK_POLL: Duration = Duration::from_millis(5);
/// Canonical manifest JSON is a few hundred bytes. A larger body is refused before it is buffered.
const MAX_MANIFEST_BYTES: usize = 4096;

/// Why an upload call was refused. Display text is static and contains no caller data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileHostError {
    /// The context or configuration is not a usable value.
    Admission,
    /// The context is not the one stored for this session or object.
    Denied,
    /// No visible session or object exists for this caller.
    NotFound,
    /// A stored part or configuration does not match, and nothing was replaced.
    Conflict,
    /// Accepting the upload would exceed the project quota.
    Quota,
    /// A size, part size, or part count limit would be exceeded.
    Limit,
    /// `complete` was called before the expected bytes were stored.
    Incomplete,
    /// The stored bytes hash to a different sha256 than the session expects.
    Checksum,
    /// The stored byte count is not the expected size and cannot be repaired in place.
    Size,
    /// The session is already complete, so it cannot be aborted or extended.
    Closed,
    /// Durable metadata or stored bytes failed an internal consistency check.
    Corrupt,
    /// The object store returned an error. The store's own text is discarded.
    Store,
    /// This backend cannot enforce a linearizable project quota, so nothing was allocated.
    Unsupported,
    /// The deployment lock was not acquired within its wait, or the lock file is not usable.
    Unavailable,
    /// The download was cancelled or its deadline passed before the object was verified.
    Stopped,
}

impl std::fmt::Display for FileHostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Admission => "admission context or upload configuration is not usable",
            Self::Denied => "admission context does not match this upload",
            Self::NotFound => "upload is not visible",
            Self::Conflict => "part or configuration conflicts with the stored upload",
            Self::Quota => "project quota would be exceeded",
            Self::Limit => "upload exceeds a size or part limit",
            Self::Incomplete => "upload parts are not complete",
            Self::Checksum => "stored bytes do not match the expected sha256",
            Self::Size => "stored bytes do not match the expected size",
            Self::Closed => "upload session is already complete",
            Self::Corrupt => "stored upload metadata is not usable",
            Self::Store => "object store operation failed",
            Self::Unsupported => "object store cannot enforce a linearizable project quota",
            Self::Unavailable => "upload lock is not available",
            Self::Stopped => "download stopped before the object was verified",
        })
    }
}

impl std::error::Error for FileHostError {}

/// Project the future control plane has already admitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectId(String);

/// Principal the future control plane has already admitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrincipalId(String);

/// Opaque grant evidence recorded on the manifest. This module does not validate it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantEvidence(String);

/// Platform-generated session id. 32 lowercase hex characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionId(String);

/// Platform-generated object key. Callers can pass one they already learned back to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectKey(String);

/// Hex sha256 over the idempotency domain, the admission pair, and the upload configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdempotencyKey(String);

impl ProjectId {
    pub fn new(value: impl Into<String>) -> Result<Self, FileHostError> {
        let value = value.into();
        if !valid_identity(&value) {
            return Err(FileHostError::Admission);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PrincipalId {
    pub fn new(value: impl Into<String>) -> Result<Self, FileHostError> {
        let value = value.into();
        if !valid_identity(&value) {
            return Err(FileHostError::Admission);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl GrantEvidence {
    pub fn new(value: impl Into<String>) -> Result<Self, FileHostError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_GRANT_BYTES || !printable_text(&value) {
            return Err(FileHostError::Admission);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl SessionId {
    /// Accept a session id this host already returned.
    ///
    /// A value that is not 32 lowercase hex characters is [`FileHostError::NotFound`].
    /// The text is not included in the error.
    pub fn parse(value: impl Into<String>) -> Result<Self, FileHostError> {
        let value = value.into();
        if !is_hex(&value, 32) {
            return Err(FileHostError::NotFound);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ObjectKey {
    /// Accept an object key this host already returned.
    ///
    /// The only accepted form is `uploads/<session>/<sha256>/original`. Anything else
    /// is [`FileHostError::NotFound`]. The text is not included in the error.
    pub fn parse(value: impl Into<String>) -> Result<Self, FileHostError> {
        let value = value.into();
        if session_id_from_object_key(&value).is_none() {
            return Err(FileHostError::NotFound);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl IdempotencyKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Trusted admission triple. Equality of project and principal is the authorization check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmissionContext {
    pub project_id: ProjectId,
    pub principal_id: PrincipalId,
    pub grant_evidence: GrantEvidence,
}

/// Caller-supplied upload configuration. The display name is metadata, never a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UploadConfig {
    pub display_name: String,
    pub expected_sha256: [u8; 32],
    pub expected_size: u64,
    pub content_type: Option<String>,
}

/// Limits for one host. Every bound is finite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostLimits {
    max_object_bytes: u64,
    max_part_bytes: u64,
    max_part_count: u32,
    max_project_bytes: u64,
}

impl HostLimits {
    /// 5 GB object, 5 GB part, 10_000 parts, 50 GB project.
    pub fn standard() -> Self {
        Self {
            max_object_bytes: MAX_OBJECT_BYTES,
            max_part_bytes: MAX_OBJECT_BYTES,
            max_part_count: MAX_PART_COUNT,
            max_project_bytes: DEFAULT_PROJECT_BYTES,
        }
    }

    pub fn try_new(
        max_object_bytes: u64,
        max_part_bytes: u64,
        max_part_count: u32,
        max_project_bytes: u64,
    ) -> Result<Self, FileHostError> {
        if max_object_bytes == 0
            || max_object_bytes > MAX_OBJECT_BYTES
            || max_part_bytes == 0
            || max_part_bytes > max_object_bytes
            || max_part_count == 0
            || max_part_count > MAX_PART_COUNT
            || max_project_bytes == 0
        {
            return Err(FileHostError::Limit);
        }
        Ok(Self {
            max_object_bytes,
            max_part_bytes,
            max_part_count,
            max_project_bytes,
        })
    }

    pub fn max_object_bytes(&self) -> u64 {
        self.max_object_bytes
    }

    pub fn max_part_bytes(&self) -> u64 {
        self.max_part_bytes
    }

    pub fn max_part_count(&self) -> u32 {
        self.max_part_count
    }

    pub fn max_project_bytes(&self) -> u64 {
        self.max_project_bytes
    }
}

/// Open session handle. A later `begin` with the same context and configuration returns the same ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UploadSession {
    pub session_id: SessionId,
    pub object_key: ObjectKey,
    pub idempotency_key: IdempotencyKey,
    pub expected_sha256: [u8; 32],
    pub expected_size: u64,
}

/// One stored part. `part_sha256` is the sha256 of that part's bytes, not of the whole object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartReceipt {
    pub part_number: u32,
    pub part_sha256: [u8; 32],
    pub size: u64,
}

/// Immutable description of a completed object, including the lineage digest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectManifest {
    pub contract: &'static str,
    pub object_key: ObjectKey,
    pub session_id: SessionId,
    pub project_id: ProjectId,
    pub principal_id: PrincipalId,
    pub grant_evidence: GrantEvidence,
    pub display_name: String,
    pub content_type: Option<String>,
    pub object_digest: [u8; 32],
    pub size: u64,
    pub part_count: u32,
    pub idempotency_key: IdempotencyKey,
    pub lineage_digest: [u8; 32],
}

/// Bytes read back from the store, together with the manifest those bytes hashed to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredObject {
    pub manifest: ObjectManifest,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SessionRecord {
    session_id: String,
    project_id: String,
    principal_id: String,
    grant_evidence: String,
    display_name: String,
    content_type: Option<String>,
    expected_sha256: [u8; 32],
    expected_size: u64,
    idempotency_key: String,
    object_key: String,
}

/// Upload sessions stored through the object store trait.
///
/// `LocalFileSystem` multipart uploads are not durable across a dropped handle.
/// Parts are therefore ordinary objects. `complete` hashes and publishes those
/// parts in chunks of at most 8192 bytes, and the final key is published only
/// after the streamed bytes hash and measure to the expected sha256 and size.
/// A failed complete does not leave that key in place.
///
/// [`FileHost::open_local`] locks a platform-generated file beside the
/// deployment root for every mutating call. [`FileHost::new`] has no lock and
/// refuses those calls instead of racing a quota check.
pub struct FileHost {
    store: Arc<dyn ObjectStore>,
    limits: HostLimits,
    deployment_lock: Option<std::path::PathBuf>,
    /// Canonical deployment directory for [`FileHost::open_local`].
    ///
    /// [`FileHost::new`] leaves this empty. A download on that host has no local
    /// stat and uses the store's ranged reads as its identity proof.
    local_root: Option<std::path::PathBuf>,
}

impl std::fmt::Debug for FileHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileHost")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl FileHost {
    /// Bind an already constructed store. The store is the uniform backend interface.
    pub fn new(store: Arc<dyn ObjectStore>, limits: HostLimits) -> Self {
        Self {
            store,
            limits,
            deployment_lock: None,
            local_root: None,
        }
    }

    /// Open a local store rooted at `root`, creating the directory if needed.
    ///
    /// `root` is the deployment prefix, not an object key. Upload methods do not
    /// take filesystem paths. Writes fsync before success, matching the local
    /// store's durability switch for remote backends.
    pub fn open_local(root: &std::path::Path, limits: HostLimits) -> Result<Self, FileHostError> {
        std::fs::create_dir_all(root).map_err(|_| FileHostError::Store)?;
        let local_root = std::fs::canonicalize(root).map_err(|_| FileHostError::Store)?;
        let lock_path = deployment_lock_path(&local_root)?;
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|_| FileHostError::Unavailable)?;
        let local = object_store::local::LocalFileSystem::new_with_prefix(&local_root)
            .map_err(|_| FileHostError::Store)?
            .with_fsync(true);
        Ok(Self {
            store: Arc::new(local),
            limits,
            deployment_lock: Some(lock_path),
            local_root: Some(local_root),
        })
    }

    pub fn limits(&self) -> HostLimits {
        self.limits
    }

    /// Start a session, or return the existing one for this context and configuration.
    pub async fn begin(
        &self,
        ctx: &AdmissionContext,
        config: &UploadConfig,
    ) -> Result<UploadSession, FileHostError> {
        let _guard = self.lock_deployment().await?;
        let validated = validate_config(config, self.limits)?;
        let idempotency = idempotency_digest(ctx, &validated);
        let idem_key = hex_encode(&idempotency);
        let pointer_key = idempotency_pointer_key(ctx, &idem_key)?;
        if let Some(existing) = self.get_bytes(&pointer_key).await? {
            let session_id = parse_session_body(&existing)?;
            let record = self.load_session(&session_id).await?;
            if !binding_matches(&record, ctx, &validated, &idem_key) {
                return Err(FileHostError::Conflict);
            }
            return Ok(session_view(&record));
        }
        self.reserve_quota(ctx, validated.expected_size).await?;
        let session_id = self.allocate_session_id().await?;
        let object_key = object_key_for(&session_id, &validated.expected_sha256);
        let record = SessionRecord {
            session_id: session_id.clone(),
            project_id: ctx.project_id.as_str().to_owned(),
            principal_id: ctx.principal_id.as_str().to_owned(),
            grant_evidence: ctx.grant_evidence.as_str().to_owned(),
            display_name: validated.display_name.clone(),
            content_type: validated.content_type.clone(),
            expected_sha256: validated.expected_sha256,
            expected_size: validated.expected_size,
            idempotency_key: idem_key.clone(),
            object_key: object_key.clone(),
        };
        let session_key = session_object_key(&session_id);
        let quota_key = quota_object_key(ctx, &session_id);
        match self
            .put_create(&session_key, &encode_session(&record))
            .await?
        {
            PutOutcome::Created => {}
            PutOutcome::Exists => {
                self.delete_key(&quota_key).await?;
                return Err(FileHostError::Store);
            }
        }
        match self
            .put_create(&quota_key, validated.expected_size.to_string().as_bytes())
            .await?
        {
            PutOutcome::Created => {}
            PutOutcome::Exists => {
                self.delete_key(&session_key).await?;
                return Err(FileHostError::Store);
            }
        }
        match self.put_create(&pointer_key, session_id.as_bytes()).await? {
            PutOutcome::Created => Ok(session_view(&record)),
            PutOutcome::Exists => {
                self.delete_key(&quota_key).await?;
                self.delete_key(&session_key).await?;
                let Some(existing) = self.get_bytes(&pointer_key).await? else {
                    return Err(FileHostError::Store);
                };
                let winner = parse_session_body(&existing)?;
                let winner_record = self.load_session(&winner).await?;
                if !binding_matches(&winner_record, ctx, &validated, &idem_key) {
                    return Err(FileHostError::Conflict);
                }
                Ok(session_view(&winner_record))
            }
        }
    }

    /// Store one part. The same bytes for the same part number are idempotent.
    /// Different bytes leave the stored part unchanged.
    pub async fn put_part(
        &self,
        ctx: &AdmissionContext,
        session_id: &SessionId,
        part_number: u32,
        bytes: &[u8],
    ) -> Result<PartReceipt, FileHostError> {
        let _guard = self.lock_deployment().await?;
        let record = self.authorized_session(ctx, session_id).await?;
        if self.manifest_exists(session_id).await? {
            return Err(FileHostError::Closed);
        }
        if part_number == 0 || part_number > self.limits.max_part_count {
            return Err(FileHostError::Limit);
        }
        let size = u64::try_from(bytes.len()).map_err(|_| FileHostError::Limit)?;
        if size == 0 || size > self.limits.max_part_bytes || size > record.expected_size {
            return Err(FileHostError::Limit);
        }
        let part_key = part_object_key(session_id, part_number)?;
        let digest = sha256(bytes);
        if self.stored_len(&part_key).await?.is_some() {
            let (stored, stored_size) = match self.hash_key(&part_key).await {
                Ok(found) => found,
                Err(FileHostError::NotFound) => return Err(FileHostError::Store),
                Err(error) => return Err(error),
            };
            return if stored == digest && stored_size == size {
                Ok(PartReceipt {
                    part_number,
                    part_sha256: digest,
                    size,
                })
            } else {
                Err(FileHostError::Conflict)
            };
        }
        let occupied = self.part_map(session_id).await?;
        let mut total = 0u64;
        for (number, meta_size) in &occupied {
            if *number == part_number {
                continue;
            }
            total = total.checked_add(*meta_size).ok_or(FileHostError::Limit)?;
        }
        if total.checked_add(size).ok_or(FileHostError::Limit)? > record.expected_size {
            return Err(FileHostError::Limit);
        }
        match self.put_create(&part_key, bytes).await? {
            PutOutcome::Created => Ok(PartReceipt {
                part_number,
                part_sha256: digest,
                size,
            }),
            PutOutcome::Exists => {
                let (stored, stored_size) = match self.hash_key(&part_key).await {
                    Ok(found) => found,
                    Err(FileHostError::NotFound) => return Err(FileHostError::Store),
                    Err(error) => return Err(error),
                };
                if stored == digest && stored_size == size {
                    Ok(PartReceipt {
                        part_number,
                        part_sha256: digest,
                        size: stored_size,
                    })
                } else {
                    Err(FileHostError::Conflict)
                }
            }
        }
    }

    /// Publish the object when the stored parts match the expected sha256 and size.
    ///
    /// A second call returns the same manifest. A checksum or unrepairable size
    /// mismatch deletes only this session's owned staging keys and does not
    /// create the final object.
    pub async fn complete(
        &self,
        ctx: &AdmissionContext,
        session_id: &SessionId,
    ) -> Result<ObjectManifest, FileHostError> {
        let _guard = self.lock_deployment().await?;
        if let Some(manifest) = self.load_manifest_if_present(session_id).await? {
            authorize_manifest(&manifest, ctx)?;
            return Ok(manifest);
        }
        let record = self.authorized_session(ctx, session_id).await?;
        let parts = self.part_map(session_id).await?;
        if !parts_are_contiguous(&parts) {
            return Err(FileHostError::Incomplete);
        }
        let mut total = 0u64;
        for size in parts.values() {
            total = total.checked_add(*size).ok_or(FileHostError::Corrupt)?;
        }
        if total < record.expected_size {
            return Err(FileHostError::Incomplete);
        }
        if total > record.expected_size {
            self.cleanup_open(&record).await?;
            return Err(FileHostError::Size);
        }
        let (digest, hashed_size) = self.hash_parts(session_id, &parts).await?;
        if hashed_size != record.expected_size {
            self.cleanup_open(&record).await?;
            return Err(FileHostError::Size);
        }
        if digest != record.expected_sha256 {
            self.cleanup_open(&record).await?;
            return Err(FileHostError::Checksum);
        }
        self.publish_parts(&record, &parts).await?;
        let (published_digest, published_size) = self.hash_object(&record.object_key).await?;
        if published_digest != digest || published_size != record.expected_size {
            self.delete_key(&record.object_key).await?;
            return Err(FileHostError::Corrupt);
        }
        let part_count = u32::try_from(parts.len()).map_err(|_| FileHostError::Corrupt)?;
        let manifest = manifest_from_record(&record, digest, published_size, part_count);
        match self
            .put_create(
                &manifest_object_key(session_id),
                &encode_manifest(&manifest),
            )
            .await?
        {
            PutOutcome::Created => {}
            PutOutcome::Exists => {
                let stored = self.load_manifest(session_id).await?;
                if stored != manifest {
                    return Err(FileHostError::Conflict);
                }
            }
        }
        self.delete_parts(session_id, &parts).await?;
        Ok(manifest)
    }

    /// Delete this session's staging keys. A completed object is left in place.
    /// A different project or principal does not delete anything.
    pub async fn abort(
        &self,
        ctx: &AdmissionContext,
        session_id: &SessionId,
    ) -> Result<(), FileHostError> {
        let _guard = self.lock_deployment().await?;
        let record = self.authorized_session(ctx, session_id).await?;
        if self.manifest_exists(session_id).await? {
            return Err(FileHostError::Closed);
        }
        self.cleanup_open(&record).await
    }

    /// Read the manifest for a platform key. A foreign context is [`FileHostError::Denied`].
    pub async fn read_manifest(
        &self,
        ctx: &AdmissionContext,
        object_key: &ObjectKey,
    ) -> Result<ObjectManifest, FileHostError> {
        let manifest = self.manifest_for_key(object_key).await?;
        authorize_manifest(&manifest, ctx)?;
        Ok(manifest)
    }

    /// Read the published bytes into one buffer and re-hash them before returning.
    ///
    /// The returned vector has the object size. [`Self::open_download`] pulls the
    /// same object in bounded ranges and does not build that buffer.
    pub async fn read_object(
        &self,
        ctx: &AdmissionContext,
        object_key: &ObjectKey,
    ) -> Result<StoredObject, FileHostError> {
        let manifest = self.read_manifest(ctx, object_key).await?;
        let Some(size) = self.stored_len(object_key.as_str()).await? else {
            return Err(FileHostError::Corrupt);
        };
        if size != manifest.size {
            return Err(FileHostError::Corrupt);
        }
        let mut hasher = Sha256::new();
        let mut bytes = Vec::new();
        if size > 0 {
            let cap = usize::try_from(size).map_err(|_| FileHostError::Corrupt)?;
            bytes.try_reserve(cap).map_err(|_| FileHostError::Store)?;
        }
        let read = self
            .for_each_chunk(object_key.as_str(), |chunk| {
                hasher.update(chunk);
                bytes.extend_from_slice(chunk);
            })
            .await?;
        if read != size || finalize(hasher) != manifest.object_digest {
            return Err(FileHostError::Corrupt);
        }
        Ok(StoredObject { manifest, bytes })
    }

    /// Open a bounded pull of one published object.
    ///
    /// `chunk_budget` must be `1..=8192`. A value outside that range is
    /// [`FileHostError::Limit`] and does not touch the store. `deadline`, when
    /// set, is a caller clock checked before and after every `get_opts` and
    /// `bytes` await in the pull, including the end-of-object rehash. An await
    /// that has already started is not cancelled. The deadline is not a TTL, a
    /// revocation check, or a credential. The admission context is trusted
    /// input already accepted by a future control step. This method compares
    /// project and principal with the published manifest. It does not speak
    /// HTTP and it does not take a filesystem path.
    pub async fn open_download(
        &self,
        ctx: &AdmissionContext,
        object_key: &ObjectKey,
        chunk_budget: usize,
        deadline: Option<std::time::Instant>,
    ) -> Result<ObjectDownload, FileHostError> {
        if chunk_budget == 0 || chunk_budget > STREAM_CHUNK_BYTES {
            return Err(FileHostError::Limit);
        }
        let Some(session_id) = session_id_from_object_key(object_key.as_str()) else {
            return Err(FileHostError::NotFound);
        };
        let manifest_key = manifest_object_key_str(&session_id);
        let Some(manifest_bytes) =
            load_capped_bytes(self.store.as_ref(), &manifest_key, MAX_MANIFEST_BYTES).await?
        else {
            return Err(FileHostError::NotFound);
        };
        let manifest = decode_manifest(&manifest_bytes)?;
        if manifest.object_key.as_str() != object_key.as_str()
            || manifest.session_id.as_str() != session_id
        {
            return Err(FileHostError::NotFound);
        }
        authorize_manifest(&manifest, ctx)?;
        if manifest.size == 0 {
            return Err(FileHostError::Corrupt);
        }
        let Some(stored) = store_len(self.store.as_ref(), object_key.as_str()).await? else {
            return Err(FileHostError::Corrupt);
        };
        if stored != manifest.size {
            return Err(FileHostError::Corrupt);
        }
        let local_file = match &self.local_root {
            Some(root) => Some(local_object_file(root, object_key.as_str())?),
            None => None,
        };
        Ok(ObjectDownload {
            store: Arc::clone(&self.store),
            location: parse_store_key(object_key.as_str())?,
            manifest_key,
            manifest,
            manifest_bytes,
            local_file,
            chunk_budget,
            deadline,
            cancel: Arc::new(AtomicBool::new(false)),
            offset: 0,
            hasher: Sha256::new(),
            complete: false,
            stop: None,
        })
    }

    async fn reserve_quota(
        &self,
        ctx: &AdmissionContext,
        expected_size: u64,
    ) -> Result<(), FileHostError> {
        let prefix = format!("meta/quota/{}", ctx.project_id.as_str());
        let locations = self.list_prefix(&prefix).await?;
        let mut used = 0u64;
        for location in locations {
            let Some(name) = location.rsplit('/').next() else {
                continue;
            };
            if !is_hex(name, 32) {
                continue;
            }
            let Some(body) = self.get_bytes(&location).await? else {
                continue;
            };
            let reserved = parse_canonical_u64(&body).ok_or(FileHostError::Corrupt)?;
            used = used.checked_add(reserved).ok_or(FileHostError::Corrupt)?;
        }
        if used
            .checked_add(expected_size)
            .ok_or(FileHostError::Limit)?
            > self.limits.max_project_bytes
        {
            return Err(FileHostError::Quota);
        }
        Ok(())
    }

    async fn allocate_session_id(&self) -> Result<String, FileHostError> {
        for _ in 0..8 {
            let mut bytes = [0u8; 16];
            getrandom::getrandom(&mut bytes).map_err(|_| FileHostError::Store)?;
            let id = hex_encode(&bytes);
            if self.get_bytes(&session_object_key(&id)).await?.is_none() {
                return Ok(id);
            }
        }
        Err(FileHostError::Store)
    }

    async fn authorized_session(
        &self,
        ctx: &AdmissionContext,
        session_id: &SessionId,
    ) -> Result<SessionRecord, FileHostError> {
        if !is_hex(session_id.as_str(), 32) {
            return Err(FileHostError::NotFound);
        }
        let Some(bytes) = self
            .get_bytes(&session_object_key(session_id.as_str()))
            .await?
        else {
            return Err(FileHostError::NotFound);
        };
        let record = decode_session(&bytes)?;
        if record.session_id != session_id.as_str() {
            return Err(FileHostError::Corrupt);
        }
        authorize_record(&record, ctx)?;
        Ok(record)
    }

    async fn load_session(&self, session_id: &str) -> Result<SessionRecord, FileHostError> {
        let Some(bytes) = self.get_bytes(&session_object_key(session_id)).await? else {
            return Err(FileHostError::Corrupt);
        };
        let record = decode_session(&bytes)?;
        if record.session_id != session_id {
            return Err(FileHostError::Corrupt);
        }
        Ok(record)
    }

    async fn manifest_exists(&self, session_id: &SessionId) -> Result<bool, FileHostError> {
        Ok(self
            .get_bytes(&manifest_object_key(session_id))
            .await?
            .is_some())
    }

    async fn load_manifest_if_present(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<ObjectManifest>, FileHostError> {
        let Some(bytes) = self.get_bytes(&manifest_object_key(session_id)).await? else {
            return Ok(None);
        };
        Ok(Some(decode_manifest(&bytes)?))
    }

    async fn load_manifest(&self, session_id: &SessionId) -> Result<ObjectManifest, FileHostError> {
        self.load_manifest_if_present(session_id)
            .await?
            .ok_or(FileHostError::Corrupt)
    }

    async fn manifest_for_key(
        &self,
        object_key: &ObjectKey,
    ) -> Result<ObjectManifest, FileHostError> {
        let Some(session_id) = session_id_from_object_key(object_key.as_str()) else {
            return Err(FileHostError::NotFound);
        };
        let Some(bytes) = self
            .get_bytes(&manifest_object_key_str(&session_id))
            .await?
        else {
            return Err(FileHostError::NotFound);
        };
        let manifest = decode_manifest(&bytes)?;
        if manifest.object_key.as_str() != object_key.as_str()
            || manifest.session_id.as_str() != session_id
        {
            return Err(FileHostError::NotFound);
        }
        Ok(manifest)
    }

    async fn part_map(&self, session_id: &SessionId) -> Result<BTreeMap<u32, u64>, FileHostError> {
        let prefix = format!("staging/{}/part", session_id.as_str());
        let locations = self.list_prefix(&prefix).await?;
        let mut parts = BTreeMap::new();
        for location in locations {
            let Some(number) = part_number_from_key(&location, session_id.as_str()) else {
                continue;
            };
            let Some(size) = self.stored_len(&location).await? else {
                return Err(FileHostError::Store);
            };
            if parts.insert(number, size).is_some() {
                return Err(FileHostError::Corrupt);
            }
        }
        Ok(parts)
    }

    async fn hash_parts(
        &self,
        session_id: &SessionId,
        parts: &BTreeMap<u32, u64>,
    ) -> Result<([u8; 32], u64), FileHostError> {
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        for number in parts.keys() {
            let key = part_object_key(session_id, *number)?;
            let size = match self
                .for_each_chunk(&key, |chunk| hasher.update(chunk))
                .await
            {
                Ok(size) => size,
                Err(FileHostError::NotFound) => return Err(FileHostError::Incomplete),
                Err(error) => return Err(error),
            };
            if Some(&size) != parts.get(number) {
                return Err(FileHostError::Corrupt);
            }
            total = total.checked_add(size).ok_or(FileHostError::Corrupt)?;
        }
        Ok((finalize(hasher), total))
    }

    async fn publish_parts(
        &self,
        record: &SessionRecord,
        parts: &BTreeMap<u32, u64>,
    ) -> Result<(), FileHostError> {
        let path = parse_store_key(&record.object_key)?;
        let mut upload = self
            .store
            .put_multipart_opts(&path, PutMultipartOptions::default())
            .await
            .map_err(|_| FileHostError::Store)?;
        for number in parts.keys() {
            let key = part_object_key_str(&record.session_id, *number)?;
            if let Err(error) = self.write_chunks(&key, upload.as_mut()).await {
                let _ = upload.abort().await;
                return Err(error);
            }
        }
        if upload.complete().await.is_err() {
            let _ = upload.abort().await;
            return Err(FileHostError::Store);
        }
        Ok(())
    }

    async fn hash_object(&self, key: &str) -> Result<([u8; 32], u64), FileHostError> {
        self.hash_key(key).await
    }

    async fn hash_key(&self, key: &str) -> Result<([u8; 32], u64), FileHostError> {
        let mut hasher = Sha256::new();
        let size = self
            .for_each_chunk(key, |chunk| hasher.update(chunk))
            .await?;
        Ok((finalize(hasher), size))
    }

    /// Visit the object in ranges of at most `STREAM_CHUNK_BYTES`.
    ///
    /// The range is chosen before `get_opts`. A backend that returns a larger buffer fails closed.
    async fn for_each_chunk(
        &self,
        key: &str,
        mut visit: impl FnMut(&[u8]),
    ) -> Result<u64, FileHostError> {
        let path = parse_store_key(key)?;
        let known = match self.store.head(&path).await {
            Ok(meta) => meta.size,
            Err(object_store::Error::NotFound { .. }) => return Err(FileHostError::NotFound),
            Err(_) => return Err(FileHostError::Store),
        };
        let mut offset = 0u64;
        while offset < known {
            let chunk =
                read_bounded_range(&*self.store, &path, offset, known, STREAM_CHUNK_BYTES).await?;
            let len = u64::try_from(chunk.len()).map_err(|_| FileHostError::Corrupt)?;
            visit(&chunk);
            offset = offset.checked_add(len).ok_or(FileHostError::Corrupt)?;
        }
        if offset != known {
            return Err(FileHostError::Corrupt);
        }
        Ok(known)
    }

    async fn write_chunks(
        &self,
        key: &str,
        upload: &mut dyn MultipartUpload,
    ) -> Result<(), FileHostError> {
        let path = parse_store_key(key)?;
        let known = match self.store.head(&path).await {
            Ok(meta) => meta.size,
            Err(object_store::Error::NotFound { .. }) => return Err(FileHostError::Incomplete),
            Err(_) => return Err(FileHostError::Store),
        };
        let mut offset = 0u64;
        while offset < known {
            let chunk =
                read_bounded_range(&*self.store, &path, offset, known, STREAM_CHUNK_BYTES).await?;
            let len = u64::try_from(chunk.len()).map_err(|_| FileHostError::Corrupt)?;
            if upload.put_part(PutPayload::from(chunk)).await.is_err() {
                return Err(FileHostError::Store);
            }
            offset = offset.checked_add(len).ok_or(FileHostError::Corrupt)?;
        }
        if offset != known {
            return Err(FileHostError::Corrupt);
        }
        Ok(())
    }

    async fn stored_len(&self, key: &str) -> Result<Option<u64>, FileHostError> {
        store_len(self.store.as_ref(), key).await
    }

    async fn lock_deployment(&self) -> Result<HeldLock, FileHostError> {
        let Some(path) = self.deployment_lock.clone() else {
            return Err(FileHostError::Unsupported);
        };
        let gate = gate_for(&path)?;
        let token = next_lock_token();
        let wait = Arc::new(LockWait {
            cancel: AtomicBool::new(false),
            slot: Mutex::new(None),
            waker: Mutex::new(None),
        });
        let thread_wait = Arc::clone(&wait);
        std::thread::Builder::new()
            .name("oo-file-host-lock".to_owned())
            .spawn(move || blocking_lock(path, gate, token, thread_wait))
            .map_err(|_| FileHostError::Unavailable)?;
        let mut cancel = CancelOnDrop {
            wait: Arc::clone(&wait),
            armed: true,
        };
        let outcome = std::future::poll_fn(|cx| {
            {
                let mut waker = lock_mutex(&wait.waker);
                *waker = Some(cx.waker().clone());
            }
            let mut slot = lock_mutex(&wait.slot);
            if let Some(outcome) = slot.take() {
                return std::task::Poll::Ready(outcome);
            }
            std::task::Poll::Pending
        })
        .await;
        cancel.armed = false;
        outcome
    }

    async fn cleanup_open(&self, record: &SessionRecord) -> Result<(), FileHostError> {
        let root = self.local_root.clone().ok_or(FileHostError::Unsupported)?;
        let session_id = SessionId(record.session_id.clone());
        let pointer = idempotency_pointer_key_str(
            &record.project_id,
            &record.principal_id,
            &record.idempotency_key,
        )?;
        let quota_key = quota_object_key_str(&record.project_id, &record.session_id);
        self.refuse_open_symlinks(&root, record, &pointer, &quota_key)
            .await?;
        let parts = self.part_map(&session_id).await?;
        self.delete_parts(&session_id, &parts).await?;
        self.reap_open_staging(&root, record, &pointer, &quota_key)?;
        if !self.open_clear(&root, record, &pointer, &quota_key).await? {
            return Err(FileHostError::Store);
        }
        self.write_open_absence(&root, record).await?;
        self.delete_key(&quota_key).await?;
        self.delete_key(&pointer).await?;
        self.delete_key(&session_object_key(&record.session_id))
            .await?;
        Ok(())
    }

    async fn delete_parts(
        &self,
        session_id: &SessionId,
        parts: &BTreeMap<u32, u64>,
    ) -> Result<(), FileHostError> {
        for number in parts.keys() {
            self.delete_key(&part_object_key(session_id, *number)?)
                .await?;
        }
        Ok(())
    }

    async fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, FileHostError> {
        let path = parse_store_key(prefix)?;
        let mut stream = self.store.list(Some(&path));
        let mut found = Vec::new();
        while let Some(item) = stream.next().await {
            let meta = item.map_err(|_| FileHostError::Store)?;
            let location = meta.location.to_string();
            if location == prefix || location.starts_with(&format!("{prefix}/")) {
                found.push(location);
            }
        }
        Ok(found)
    }

    async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, FileHostError> {
        store_bytes(self.store.as_ref(), key).await
    }

    async fn put_create(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome, FileHostError> {
        let path = parse_store_key(key)?;
        let payload: PutPayload = bytes.to_vec().into();
        let opts = PutOptions {
            mode: PutMode::Create,
            ..PutOptions::default()
        };
        match self.store.put_opts(&path, payload, opts).await {
            Ok(_) => Ok(PutOutcome::Created),
            Err(object_store::Error::AlreadyExists { .. }) => Ok(PutOutcome::Exists),
            Err(_) => Err(FileHostError::Store),
        }
    }

    async fn delete_key(&self, key: &str) -> Result<(), FileHostError> {
        let path = parse_store_key(key)?;
        match self.store.delete(&path).await {
            Ok(()) => Ok(()),
            Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(_) => Err(FileHostError::Store),
        }
    }

    async fn put_overwrite(&self, key: &str, bytes: &[u8]) -> Result<(), FileHostError> {
        let path = parse_store_key(key)?;
        let payload: PutPayload = bytes.to_vec().into();
        self.store
            .put(&path, payload)
            .await
            .map_err(|_| FileHostError::Store)?;
        Ok(())
    }

    /// Delete one completed object under the deployment lock.
    ///
    /// The object key, digest, size, lineage, project, principal, and grant must
    /// match the stored manifest. `stop_after_data_absent` returns `Ok(None)`
    /// only after the bytes, sidecars, parts, and platform staging are durable
    /// and absent while the quota key is still held. `false` continues through
    /// the single quota release and `verified`. `Ok(Some)` is that verified
    /// delete. Neither value releases quota while those bytes are still present.
    /// `abort` of a completed session is unchanged and still deletes nothing.
    pub async fn delete_completed(
        &self,
        ctx: &AdmissionContext,
        object_key: &ObjectKey,
        object_digest: [u8; 32],
        size: u64,
        lineage_digest: [u8; 32],
        stop_after_data_absent: bool,
    ) -> Result<Option<ObjectManifest>, FileHostError> {
        let _guard = self.lock_deployment().await?;
        let root = self.local_root.clone().ok_or(FileHostError::Unsupported)?;
        let target = CompletedDelete {
            ctx,
            object_key,
            object_digest,
            size,
            lineage_digest,
        };
        self.delete_completed_locked(&root, &target, stop_after_data_absent)
            .await
    }

    /// Prove an open abort removed this session's parts and platform staging.
    ///
    /// This call writes nothing. `Ok(true)` requires the open-absence receipt,
    /// the same project, principal, and grant, and no five-digit part or
    /// platform staging file. A missing receipt is [`FileHostError::NotFound`].
    /// `read_object` returning `NotFound` is not this proof.
    pub async fn observe_open_absence(
        &self,
        ctx: &AdmissionContext,
        session_id: &SessionId,
    ) -> Result<bool, FileHostError> {
        let _guard = self.lock_deployment().await?;
        let root = self.local_root.clone().ok_or(FileHostError::Unsupported)?;
        self.observe_open_absence_locked(&root, ctx, session_id)
            .await
    }

    /// Observe whether a verified delete left the object, manifest, and
    /// five-digit parts absent. This call writes nothing. `Ok(true)` is the
    /// only absence proof. A store error is [`FileHostError::Store`].
    pub async fn observe_completed_absence(
        &self,
        ctx: &AdmissionContext,
        object_key: &ObjectKey,
        object_digest: [u8; 32],
        size: u64,
        lineage_digest: [u8; 32],
    ) -> Result<bool, FileHostError> {
        let _guard = self.lock_deployment().await?;
        let root = self.local_root.clone().ok_or(FileHostError::Unsupported)?;
        self.observe_absence_locked(&root, ctx, object_key, object_digest, size, lineage_digest)
            .await
    }

    /// How many times this completed delete released the object's quota key.
    ///
    /// The result is `0` or `1`. A missing tombstone is `0` and does not prove
    /// that the object exists.
    pub async fn completed_quota_release_count(
        &self,
        ctx: &AdmissionContext,
        object_key: &ObjectKey,
    ) -> Result<u8, FileHostError> {
        let _guard = self.lock_deployment().await?;
        let root = self.local_root.clone().ok_or(FileHostError::Unsupported)?;
        let session_id =
            session_id_from_object_key(object_key.as_str()).ok_or(FileHostError::NotFound)?;
        let Some(tomb) = self.read_tombstone(&root, &session_id).await? else {
            return Ok(0);
        };
        if tomb.manifest.project_id.as_str() != ctx.project_id.as_str()
            || tomb.manifest.principal_id.as_str() != ctx.principal_id.as_str()
            || tomb.manifest.grant_evidence.as_str() != ctx.grant_evidence.as_str()
        {
            return Err(FileHostError::Denied);
        }
        if tomb.manifest.object_key.as_str() != object_key.as_str() {
            return Err(FileHostError::NotFound);
        }
        Ok(tomb.release_count)
    }

    async fn delete_completed_locked(
        &self,
        root: &std::path::Path,
        target: &CompletedDelete<'_>,
        stop_after_data_absent: bool,
    ) -> Result<Option<ObjectManifest>, FileHostError> {
        let session_id = require_digest_key(target.object_key, &target.object_digest)?;
        let existing = self.read_tombstone(root, &session_id).await?;
        let snapshot = match &existing {
            Some(tomb) => {
                precondition_matches(
                    &tomb.manifest,
                    target.ctx,
                    target.object_key,
                    &target.object_digest,
                    target.size,
                    &target.lineage_digest,
                )?;
                tomb.manifest.clone()
            }
            None => self.fresh_snapshot(root, target, &session_id).await?,
        };
        if let Some(manifest) = self.load_manifest_if_file(root, &session_id).await? {
            precondition_matches(
                &manifest,
                target.ctx,
                target.object_key,
                &target.object_digest,
                target.size,
                &target.lineage_digest,
            )?;
            if manifest != snapshot {
                return Err(FileHostError::Conflict);
            }
        } else if existing.is_none() {
            return Err(FileHostError::NotFound);
        }
        self.session_agrees(root, &session_id, &snapshot).await?;
        let quota_key = quota_object_key_str(snapshot.project_id.as_str(), &session_id);
        let quota_held = self.quota_matches(root, &quota_key, snapshot.size).await?;
        let pointer = idempotency_pointer_key_str(
            snapshot.project_id.as_str(),
            snapshot.principal_id.as_str(),
            snapshot.idempotency_key.as_str(),
        )?;
        let reused_slot = existing
            .as_ref()
            .is_some_and(|tomb| tomb.phase == TombPhase::Verified);
        self.pointer_agrees(root, &pointer, &session_id, reused_slot)
            .await?;
        let track = self
            .prepare_delete(
                root,
                existing,
                &snapshot,
                &session_id,
                &quota_key,
                quota_held,
            )
            .await?;
        match track {
            DeleteTrack::Done(tomb) => {
                self.verified_result(root, &session_id, target.object_key, &pointer, &tomb)
                    .await
            }
            DeleteTrack::Legacy(tomb) => {
                self.finish_legacy(root, &session_id, target.object_key, &pointer, &tomb)
                    .await
            }
            DeleteTrack::Hold(tomb) => {
                self.finish_bytes_then_quota(
                    root,
                    &session_id,
                    target.object_key,
                    &pointer,
                    &tomb,
                    stop_after_data_absent,
                )
                .await
            }
        }
    }

    async fn fresh_snapshot(
        &self,
        root: &std::path::Path,
        target: &CompletedDelete<'_>,
        session_id: &str,
    ) -> Result<ObjectManifest, FileHostError> {
        let Some(manifest) = self.load_manifest_if_file(root, session_id).await? else {
            return Err(FileHostError::NotFound);
        };
        precondition_matches(
            &manifest,
            target.ctx,
            target.object_key,
            &target.object_digest,
            target.size,
            &target.lineage_digest,
        )?;
        Ok(manifest)
    }

    async fn prepare_delete(
        &self,
        root: &std::path::Path,
        existing: Option<Tombstone>,
        snapshot: &ObjectManifest,
        session_id: &str,
        quota_key: &str,
        quota_held: bool,
    ) -> Result<DeleteTrack, FileHostError> {
        let Some(tomb) = existing else {
            if !quota_held {
                return Err(FileHostError::Conflict);
            }
            self.refuse_odd_tree(root, snapshot, session_id, quota_key)
                .await?;
            let deleting = tombstone_at(
                snapshot,
                quota_key,
                TombPhase::DataDeleting,
                0,
                TombOrder::BytesThenQuota,
            );
            self.write_tombstone(session_id, &deleting).await?;
            return Ok(DeleteTrack::Hold(deleting));
        };
        match (tomb.phase, tomb.order, tomb.release_count, quota_held) {
            (TombPhase::Verified, _, 1, _) => Ok(DeleteTrack::Done(tomb)),
            (TombPhase::Intent, TombOrder::Legacy, 0, true) => {
                self.refuse_odd_tree(root, snapshot, session_id, quota_key)
                    .await?;
                let deleting = tombstone_at(
                    snapshot,
                    quota_key,
                    TombPhase::DataDeleting,
                    0,
                    TombOrder::BytesThenQuota,
                );
                self.write_tombstone(session_id, &deleting).await?;
                Ok(DeleteTrack::Hold(deleting))
            }
            (TombPhase::Intent, TombOrder::Legacy, 0, false) => {
                let reconciled = tombstone_at(
                    snapshot,
                    quota_key,
                    TombPhase::QuotaReleased,
                    1,
                    TombOrder::LegacyReconcile,
                );
                self.write_tombstone(session_id, &reconciled).await?;
                Ok(DeleteTrack::Legacy(reconciled))
            }
            (TombPhase::DataDeleting, TombOrder::BytesThenQuota, 0, true)
            | (TombPhase::DataAbsent, TombOrder::BytesThenQuota, 0, _) => {
                Ok(DeleteTrack::Hold(tomb))
            }
            (TombPhase::DataDeleting, TombOrder::BytesThenQuota, 0, false) => {
                Err(FileHostError::Conflict)
            }
            (TombPhase::QuotaReleased, TombOrder::BytesThenQuota, 1, false) => {
                Ok(DeleteTrack::Hold(tomb))
            }
            (TombPhase::QuotaReleased, TombOrder::BytesThenQuota, 1, true) => {
                Err(FileHostError::Conflict)
            }
            (TombPhase::QuotaReleased, TombOrder::Legacy, 1, _) => {
                let reconciled = tombstone_at(
                    snapshot,
                    quota_key,
                    TombPhase::QuotaReleased,
                    1,
                    TombOrder::LegacyReconcile,
                );
                self.write_tombstone(session_id, &reconciled).await?;
                Ok(DeleteTrack::Legacy(reconciled))
            }
            (TombPhase::QuotaReleased, TombOrder::LegacyReconcile, 1, _) => {
                Ok(DeleteTrack::Legacy(tomb))
            }
            _ => Err(FileHostError::Corrupt),
        }
    }

    async fn finish_legacy(
        &self,
        root: &std::path::Path,
        session_id: &str,
        object_key: &ObjectKey,
        pointer: &str,
        tomb: &Tombstone,
    ) -> Result<Option<ObjectManifest>, FileHostError> {
        self.remove_owned(root, session_id, object_key, pointer)
            .await?;
        if !self
            .owned_absent(root, session_id, object_key, pointer)
            .await?
        {
            return Err(FileHostError::Store);
        }
        let verified = tombstone_at(
            &tomb.manifest,
            &tomb.quota_key,
            TombPhase::Verified,
            1,
            TombOrder::LegacyReconcile,
        );
        self.write_tombstone(session_id, &verified).await?;
        Ok(Some(tomb.manifest.clone()))
    }

    async fn finish_bytes_then_quota(
        &self,
        root: &std::path::Path,
        session_id: &str,
        object_key: &ObjectKey,
        pointer: &str,
        tomb: &Tombstone,
        stop_after_data_absent: bool,
    ) -> Result<Option<ObjectManifest>, FileHostError> {
        let quota_key = tomb.quota_key.as_str();
        if tomb.phase == TombPhase::QuotaReleased {
            self.remove_owned(root, session_id, object_key, pointer)
                .await?;
            if !self
                .owned_absent(root, session_id, object_key, pointer)
                .await?
            {
                return Err(FileHostError::Store);
            }
            let verified = tombstone_at(
                &tomb.manifest,
                quota_key,
                TombPhase::Verified,
                1,
                TombOrder::BytesThenQuota,
            );
            self.write_tombstone(session_id, &verified).await?;
            return Ok(Some(tomb.manifest.clone()));
        }
        if tomb.phase == TombPhase::DataDeleting || tomb.phase == TombPhase::DataAbsent {
            self.remove_owned(root, session_id, object_key, pointer)
                .await?;
            if !self
                .owned_absent(root, session_id, object_key, pointer)
                .await?
            {
                return Err(FileHostError::Store);
            }
        } else {
            return Err(FileHostError::Corrupt);
        }
        if tomb.phase == TombPhase::DataDeleting {
            let absent = tombstone_at(
                &tomb.manifest,
                quota_key,
                TombPhase::DataAbsent,
                0,
                TombOrder::BytesThenQuota,
            );
            self.write_tombstone(session_id, &absent).await?;
        }
        let held = self
            .quota_matches(root, quota_key, tomb.manifest.size)
            .await?;
        if !held {
            let released = tombstone_at(
                &tomb.manifest,
                quota_key,
                TombPhase::QuotaReleased,
                1,
                TombOrder::BytesThenQuota,
            );
            self.write_tombstone(session_id, &released).await?;
            if !self
                .owned_absent(root, session_id, object_key, pointer)
                .await?
            {
                return Err(FileHostError::Conflict);
            }
            let verified = tombstone_at(
                &tomb.manifest,
                quota_key,
                TombPhase::Verified,
                1,
                TombOrder::BytesThenQuota,
            );
            self.write_tombstone(session_id, &verified).await?;
            return Ok(Some(tomb.manifest.clone()));
        }
        if stop_after_data_absent {
            return Ok(None);
        }
        self.delete_quota_once(root, quota_key).await?;
        if self
            .quota_matches(root, quota_key, tomb.manifest.size)
            .await?
        {
            return Err(FileHostError::Store);
        }
        let released = tombstone_at(
            &tomb.manifest,
            quota_key,
            TombPhase::QuotaReleased,
            1,
            TombOrder::BytesThenQuota,
        );
        self.write_tombstone(session_id, &released).await?;
        if !self
            .owned_absent(root, session_id, object_key, pointer)
            .await?
        {
            self.remove_owned(root, session_id, object_key, pointer)
                .await?;
            if !self
                .owned_absent(root, session_id, object_key, pointer)
                .await?
            {
                return Err(FileHostError::Conflict);
            }
        }
        let verified = tombstone_at(
            &tomb.manifest,
            quota_key,
            TombPhase::Verified,
            1,
            TombOrder::BytesThenQuota,
        );
        self.write_tombstone(session_id, &verified).await?;
        Ok(Some(tomb.manifest.clone()))
    }

    async fn verified_result(
        &self,
        root: &std::path::Path,
        session_id: &str,
        object_key: &ObjectKey,
        pointer: &str,
        tomb: &Tombstone,
    ) -> Result<Option<ObjectManifest>, FileHostError> {
        if tomb.phase != TombPhase::Verified || tomb.release_count != 1 {
            return Err(FileHostError::Corrupt);
        }
        if !self
            .owned_absent(root, session_id, object_key, pointer)
            .await?
        {
            return Err(FileHostError::Conflict);
        }
        let held = self
            .quota_matches(root, &tomb.quota_key, tomb.quota_bytes)
            .await?;
        if tomb.order == TombOrder::BytesThenQuota && held {
            return Err(FileHostError::Conflict);
        }
        Ok(Some(tomb.manifest.clone()))
    }

    async fn delete_quota_once(
        &self,
        root: &std::path::Path,
        quota_key: &str,
    ) -> Result<(), FileHostError> {
        if self.key_present(root, quota_key).await? {
            self.delete_key(quota_key).await?;
        }
        Ok(())
    }

    async fn remove_owned(
        &self,
        root: &std::path::Path,
        session_id: &str,
        object_key: &ObjectKey,
        pointer: &str,
    ) -> Result<(), FileHostError> {
        let parts = self.part_keys(root, session_id).await?;
        for key in &parts {
            self.delete_if_file(root, key).await?;
        }
        reap_part_staging(root, session_id)?;
        let manifest = manifest_object_key_str(session_id);
        let session = session_object_key(session_id);
        let tomb = tombstone_key(session_id);
        for key in [
            object_key.as_str(),
            manifest.as_str(),
            session.as_str(),
            pointer,
            tomb.as_str(),
        ] {
            reap_key_staging(root, key)?;
        }
        self.delete_if_file(root, object_key.as_str()).await?;
        self.delete_if_file(root, &manifest).await?;
        self.delete_if_file(root, &session).await?;
        self.delete_if_file(root, pointer).await?;
        Ok(())
    }

    async fn delete_if_file(&self, root: &std::path::Path, key: &str) -> Result<(), FileHostError> {
        if self.key_present(root, key).await? {
            self.delete_key(key).await?;
        }
        Ok(())
    }

    async fn observe_absence_locked(
        &self,
        root: &std::path::Path,
        ctx: &AdmissionContext,
        object_key: &ObjectKey,
        object_digest: [u8; 32],
        size: u64,
        lineage_digest: [u8; 32],
    ) -> Result<bool, FileHostError> {
        let session_id = require_digest_key(object_key, &object_digest)?;
        let tomb = self.read_tombstone(root, &session_id).await?;
        let manifest = self.load_manifest_if_file(root, &session_id).await?;
        let Some(tomb) = tomb else {
            let Some(manifest) = manifest else {
                return Err(FileHostError::NotFound);
            };
            precondition_matches(
                &manifest,
                ctx,
                object_key,
                &object_digest,
                size,
                &lineage_digest,
            )?;
            let pointer = idempotency_pointer_key_str(
                manifest.project_id.as_str(),
                manifest.principal_id.as_str(),
                manifest.idempotency_key.as_str(),
            )?;
            self.owned_absent(root, &session_id, object_key, &pointer)
                .await?;
            return Ok(false);
        };
        precondition_matches(
            &tomb.manifest,
            ctx,
            object_key,
            &object_digest,
            size,
            &lineage_digest,
        )?;
        if let Some(manifest) = manifest {
            precondition_matches(
                &manifest,
                ctx,
                object_key,
                &object_digest,
                size,
                &lineage_digest,
            )?;
            if manifest != tomb.manifest {
                return Err(FileHostError::Conflict);
            }
        }
        let pointer = idempotency_pointer_key_str(
            tomb.manifest.project_id.as_str(),
            tomb.manifest.principal_id.as_str(),
            tomb.manifest.idempotency_key.as_str(),
        )?;
        let absent = self
            .owned_absent(root, &session_id, object_key, &pointer)
            .await?;
        let quota_key = quota_object_key_str(tomb.manifest.project_id.as_str(), &session_id);
        let quota_held = self
            .quota_matches(root, &quota_key, tomb.manifest.size)
            .await?;
        match tomb.phase {
            TombPhase::Verified if absent && tomb.release_count == 1 => {
                if tomb.order == TombOrder::BytesThenQuota && quota_held {
                    Err(FileHostError::Conflict)
                } else {
                    Ok(true)
                }
            }
            TombPhase::Verified => Err(FileHostError::Corrupt),
            TombPhase::DataAbsent if !absent => Err(FileHostError::Corrupt),
            _ => Ok(false),
        }
    }

    async fn owned_absent(
        &self,
        root: &std::path::Path,
        session_id: &str,
        object_key: &ObjectKey,
        pointer: &str,
    ) -> Result<bool, FileHostError> {
        let manifest = manifest_object_key_str(session_id);
        let session = session_object_key(session_id);
        let tomb = tombstone_key(session_id);
        for key in [object_key.as_str(), manifest.as_str(), session.as_str()] {
            if self.key_present(root, key).await? || !staging_absent(root, key)? {
                return Ok(false);
            }
        }
        if self.pointer_blocks(root, pointer, session_id).await? || !staging_absent(root, pointer)?
        {
            return Ok(false);
        }
        if !staging_absent(root, &tomb)? {
            return Ok(false);
        }
        if !self.part_keys(root, session_id).await?.is_empty() || !part_dir_clear(root, session_id)?
        {
            return Ok(false);
        }
        Ok(true)
    }

    async fn refuse_odd_tree(
        &self,
        root: &std::path::Path,
        snapshot: &ObjectManifest,
        session_id: &str,
        quota_key: &str,
    ) -> Result<(), FileHostError> {
        let pointer = idempotency_pointer_key_str(
            snapshot.project_id.as_str(),
            snapshot.principal_id.as_str(),
            snapshot.idempotency_key.as_str(),
        )?;
        let mut keys = vec![
            quota_key.to_owned(),
            snapshot.object_key.as_str().to_owned(),
            manifest_object_key_str(session_id),
            session_object_key(session_id),
            pointer,
            tombstone_key(session_id),
        ];
        keys.extend(self.part_keys(root, session_id).await?);
        for key in &keys {
            let _ = self.key_present(root, key).await?;
            refuse_staging_symlink(root, key)?;
        }
        refuse_part_symlinks(root, session_id)?;
        Ok(())
    }

    async fn session_agrees(
        &self,
        root: &std::path::Path,
        session_id: &str,
        snapshot: &ObjectManifest,
    ) -> Result<(), FileHostError> {
        let key = session_object_key(session_id);
        if !self.key_present(root, &key).await? {
            return Ok(());
        }
        let Some(bytes) = load_capped_bytes(self.store.as_ref(), &key, MAX_MANIFEST_BYTES).await?
        else {
            return Err(FileHostError::Corrupt);
        };
        let record = decode_session(&bytes)?;
        if record.session_id != session_id {
            return Err(FileHostError::Corrupt);
        }
        let same = record.project_id == snapshot.project_id.as_str()
            && record.principal_id == snapshot.principal_id.as_str()
            && record.grant_evidence == snapshot.grant_evidence.as_str()
            && record.object_key == snapshot.object_key.as_str()
            && record.expected_sha256 == snapshot.object_digest
            && record.expected_size == snapshot.size;
        if same {
            Ok(())
        } else {
            Err(FileHostError::Conflict)
        }
    }

    async fn quota_matches(
        &self,
        root: &std::path::Path,
        quota_key: &str,
        size: u64,
    ) -> Result<bool, FileHostError> {
        if !self.key_present(root, quota_key).await? {
            return Ok(false);
        }
        let Some(bytes) = load_capped_bytes(self.store.as_ref(), quota_key, 32).await? else {
            return Err(FileHostError::Corrupt);
        };
        if bytes == size.to_string().as_bytes() {
            Ok(true)
        } else {
            Err(FileHostError::Conflict)
        }
    }

    async fn pointer_agrees(
        &self,
        root: &std::path::Path,
        pointer: &str,
        session_id: &str,
        allow_later_session: bool,
    ) -> Result<(), FileHostError> {
        if !self.key_present(root, pointer).await? {
            return Ok(());
        }
        let Some(bytes) = load_capped_bytes(self.store.as_ref(), pointer, 64).await? else {
            return Err(FileHostError::Corrupt);
        };
        if bytes == session_id.as_bytes() {
            return Ok(());
        }
        let later =
            allow_later_session && std::str::from_utf8(&bytes).is_ok_and(|text| is_hex(text, 32));
        if later {
            Ok(())
        } else {
            Err(FileHostError::Conflict)
        }
    }

    /// The idempotency slot still names this session.
    ///
    /// A later `begin` of the same configuration may reuse the slot after this
    /// delete has removed it. That later body is not this object's residue.
    async fn pointer_blocks(
        &self,
        root: &std::path::Path,
        pointer: &str,
        session_id: &str,
    ) -> Result<bool, FileHostError> {
        if !self.key_present(root, pointer).await? {
            return Ok(false);
        }
        let Some(bytes) = load_capped_bytes(self.store.as_ref(), pointer, 64).await? else {
            return Err(FileHostError::Corrupt);
        };
        Ok(bytes == session_id.as_bytes())
    }

    async fn load_manifest_if_file(
        &self,
        root: &std::path::Path,
        session_id: &str,
    ) -> Result<Option<ObjectManifest>, FileHostError> {
        let key = manifest_object_key_str(session_id);
        if !self.key_present(root, &key).await? {
            return Ok(None);
        }
        let Some(bytes) = load_capped_bytes(self.store.as_ref(), &key, MAX_MANIFEST_BYTES).await?
        else {
            return Err(FileHostError::Corrupt);
        };
        Ok(Some(decode_manifest(&bytes)?))
    }

    async fn part_keys(
        &self,
        root: &std::path::Path,
        session_id: &str,
    ) -> Result<Vec<String>, FileHostError> {
        let prefix = format!("staging/{session_id}/part");
        let locations = self.list_prefix(&prefix).await?;
        let mut keys = Vec::new();
        for location in locations {
            let Some(number) = part_number_from_key(&location, session_id) else {
                continue;
            };
            let key = part_object_key_str(session_id, number)?;
            if self.key_present(root, &key).await? {
                keys.push(key);
            }
        }
        Ok(keys)
    }

    async fn key_present(&self, root: &std::path::Path, key: &str) -> Result<bool, FileHostError> {
        let local_file = local_node(root, key)?;
        let stored = self.stored_len(key).await?.is_some();
        match (local_file, stored) {
            (false, false) => Ok(false),
            (true, true) => Ok(true),
            _ => Err(FileHostError::Corrupt),
        }
    }

    async fn read_tombstone(
        &self,
        root: &std::path::Path,
        session_id: &str,
    ) -> Result<Option<Tombstone>, FileHostError> {
        let key = tombstone_key(session_id);
        if !self.key_present(root, &key).await? {
            return Ok(None);
        }
        let Some(bytes) = load_capped_bytes(self.store.as_ref(), &key, MAX_MANIFEST_BYTES).await?
        else {
            return Err(FileHostError::Corrupt);
        };
        Ok(Some(decode_tombstone(&bytes)?))
    }

    async fn write_tombstone(
        &self,
        session_id: &str,
        tomb: &Tombstone,
    ) -> Result<(), FileHostError> {
        let key = tombstone_key(session_id);
        let root = self.local_root.clone().ok_or(FileHostError::Unsupported)?;
        let previous = self.read_tombstone(&root, session_id).await?;
        if !phase_transition_ok(previous.as_ref(), tomb) {
            return Err(FileHostError::Corrupt);
        }
        reap_key_staging(&root, &key)?;
        local_node(&root, &key)?;
        let bytes = encode_tombstone(tomb)?;
        self.put_overwrite(&key, &bytes).await
    }

    async fn observe_open_absence_locked(
        &self,
        root: &std::path::Path,
        ctx: &AdmissionContext,
        session_id: &SessionId,
    ) -> Result<bool, FileHostError> {
        let key = open_absence_key(session_id.as_str());
        if !self.key_present(root, &key).await? {
            return Err(FileHostError::NotFound);
        }
        let Some(bytes) = load_capped_bytes(self.store.as_ref(), &key, MAX_MANIFEST_BYTES).await?
        else {
            return Err(FileHostError::Corrupt);
        };
        let receipt = decode_open_absence(&bytes)?;
        if receipt.session != session_id.as_str() {
            return Err(FileHostError::Corrupt);
        }
        if receipt.project != ctx.project_id.as_str()
            || receipt.principal != ctx.principal_id.as_str()
            || receipt.grant != ctx.grant_evidence.as_str()
        {
            return Err(FileHostError::Denied);
        }
        let session = self
            .session_record_if_present(root, session_id.as_str())
            .await?;
        if let Some(record) = &session
            && (record.expected_size != receipt.expected_size
                || record.expected_sha256 != receipt.expected_digest
                || record.object_key != receipt.object_key
                || record.project_id != receipt.project
                || record.principal_id != receipt.principal
                || record.grant_evidence != receipt.grant)
        {
            return Err(FileHostError::Conflict);
        }
        let pointer = session
            .as_ref()
            .map(|record| {
                idempotency_pointer_key_str(
                    &record.project_id,
                    &record.principal_id,
                    &record.idempotency_key,
                )
            })
            .transpose()?;
        let quota_key = quota_object_key_str(&receipt.project, session_id.as_str());
        self.open_clear_keys(
            root,
            session_id.as_str(),
            &receipt.object_key,
            pointer.as_deref(),
            &quota_key,
        )
        .await
    }

    async fn session_record_if_present(
        &self,
        root: &std::path::Path,
        session_id: &str,
    ) -> Result<Option<SessionRecord>, FileHostError> {
        let key = session_object_key(session_id);
        if !self.key_present(root, &key).await? {
            return Ok(None);
        }
        let Some(bytes) = load_capped_bytes(self.store.as_ref(), &key, MAX_MANIFEST_BYTES).await?
        else {
            return Err(FileHostError::Corrupt);
        };
        Ok(Some(decode_session(&bytes)?))
    }

    async fn refuse_open_symlinks(
        &self,
        root: &std::path::Path,
        record: &SessionRecord,
        pointer: &str,
        quota_key: &str,
    ) -> Result<(), FileHostError> {
        let manifest = manifest_object_key_str(&record.session_id);
        let session = session_object_key(&record.session_id);
        for key in [
            record.object_key.as_str(),
            manifest.as_str(),
            session.as_str(),
            pointer,
            quota_key,
        ] {
            let _ = self.key_present(root, key).await?;
            refuse_staging_symlink(root, key)?;
        }
        refuse_part_symlinks(root, &record.session_id)?;
        let _ = self.part_keys(root, &record.session_id).await?;
        Ok(())
    }

    fn reap_open_staging(
        &self,
        root: &std::path::Path,
        record: &SessionRecord,
        pointer: &str,
        quota_key: &str,
    ) -> Result<(), FileHostError> {
        reap_part_staging(root, &record.session_id)?;
        let manifest = manifest_object_key_str(&record.session_id);
        let session = session_object_key(&record.session_id);
        for key in [
            record.object_key.as_str(),
            manifest.as_str(),
            session.as_str(),
            pointer,
            quota_key,
        ] {
            reap_key_staging(root, key)?;
        }
        Ok(())
    }

    async fn open_clear(
        &self,
        root: &std::path::Path,
        record: &SessionRecord,
        pointer: &str,
        quota_key: &str,
    ) -> Result<bool, FileHostError> {
        self.open_clear_keys(
            root,
            &record.session_id,
            &record.object_key,
            Some(pointer),
            quota_key,
        )
        .await
    }

    async fn open_clear_keys(
        &self,
        root: &std::path::Path,
        session_id: &str,
        object_key: &str,
        pointer: Option<&str>,
        quota_key: &str,
    ) -> Result<bool, FileHostError> {
        if !self.part_keys(root, session_id).await?.is_empty() || !part_dir_clear(root, session_id)?
        {
            return Ok(false);
        }
        let manifest = manifest_object_key_str(session_id);
        let session = session_object_key(session_id);
        let mut keys = vec![
            object_key.to_owned(),
            manifest,
            session,
            quota_key.to_owned(),
        ];
        if let Some(pointer) = pointer {
            keys.push(pointer.to_owned());
        }
        for key in &keys {
            if !staging_absent(root, key)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn write_open_absence(
        &self,
        root: &std::path::Path,
        record: &SessionRecord,
    ) -> Result<(), FileHostError> {
        let key = open_absence_key(&record.session_id);
        let bytes = encode_open_absence(record)?;
        if self.key_present(root, &key).await? {
            let Some(existing) =
                load_capped_bytes(self.store.as_ref(), &key, MAX_MANIFEST_BYTES).await?
            else {
                return Err(FileHostError::Corrupt);
            };
            if existing.as_slice() != bytes.as_slice() {
                return Err(FileHostError::Conflict);
            }
            return Ok(());
        }
        local_node(root, &key)?;
        self.put_overwrite(&key, &bytes).await
    }
}

enum PutOutcome {
    Created,
    Exists,
}

struct CompletedDelete<'a> {
    ctx: &'a AdmissionContext,
    object_key: &'a ObjectKey,
    object_digest: [u8; 32],
    size: u64,
    lineage_digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TombPhase {
    Intent,
    DataDeleting,
    DataAbsent,
    QuotaReleased,
    Verified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TombOrder {
    Legacy,
    BytesThenQuota,
    LegacyReconcile,
}

struct Tombstone {
    phase: TombPhase,
    manifest: ObjectManifest,
    quota_key: String,
    quota_bytes: u64,
    release_count: u8,
    order: TombOrder,
}

enum DeleteTrack {
    Done(Tombstone),
    Hold(Tombstone),
    Legacy(Tombstone),
}

fn tombstone_at(
    manifest: &ObjectManifest,
    quota_key: &str,
    phase: TombPhase,
    release_count: u8,
    order: TombOrder,
) -> Tombstone {
    Tombstone {
        phase,
        manifest: manifest.clone(),
        quota_key: quota_key.to_owned(),
        quota_bytes: manifest.size,
        release_count,
        order,
    }
}

fn tombstone_key(session_id: &str) -> String {
    format!("meta/tombstone/{session_id}")
}

fn require_digest_key(object_key: &ObjectKey, digest: &[u8; 32]) -> Result<String, FileHostError> {
    let session_id =
        session_id_from_object_key(object_key.as_str()).ok_or(FileHostError::NotFound)?;
    let embedded = object_key
        .as_str()
        .split('/')
        .nth(2)
        .ok_or(FileHostError::NotFound)?;
    if embedded == hex_encode(digest) {
        Ok(session_id)
    } else {
        Err(FileHostError::NotFound)
    }
}

fn precondition_matches(
    manifest: &ObjectManifest,
    ctx: &AdmissionContext,
    object_key: &ObjectKey,
    object_digest: &[u8; 32],
    size: u64,
    lineage_digest: &[u8; 32],
) -> Result<(), FileHostError> {
    if size == 0
        || manifest.object_key.as_str() != object_key.as_str()
        || manifest.object_digest != *object_digest
        || manifest.size != size
        || manifest.lineage_digest != *lineage_digest
    {
        return Err(FileHostError::NotFound);
    }
    if manifest.project_id.as_str() != ctx.project_id.as_str()
        || manifest.principal_id.as_str() != ctx.principal_id.as_str()
    {
        return Err(FileHostError::Denied);
    }
    if manifest.grant_evidence.as_str() == ctx.grant_evidence.as_str() {
        Ok(())
    } else {
        Err(FileHostError::Denied)
    }
}

fn local_node(root: &std::path::Path, key: &str) -> Result<bool, FileHostError> {
    if key.contains('\0')
        || key
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(FileHostError::Store);
    }
    let mut path = root.to_path_buf();
    let parts: Vec<&str> = key.split('/').collect();
    let Some((last, parents)) = parts.split_last() else {
        return Err(FileHostError::Store);
    };
    for part in parents {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(FileHostError::Corrupt),
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(FileHostError::Corrupt),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(FileHostError::Store),
        }
    }
    path.push(last);
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(FileHostError::Corrupt),
        Ok(meta) if meta.is_file() => Ok(true),
        Ok(_) => Err(FileHostError::Corrupt),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(FileHostError::Store),
    }
}

fn phase_transition_ok(from: Option<&Tombstone>, to: &Tombstone) -> bool {
    if to.release_count > 1 || to.order == TombOrder::Legacy {
        return false;
    }
    let Some(from) = from else {
        return to.phase == TombPhase::DataDeleting
            && to.release_count == 0
            && to.order == TombOrder::BytesThenQuota;
    };
    if to.release_count < from.release_count {
        return false;
    }
    matches!(
        (
            from.phase,
            from.order,
            from.release_count,
            to.phase,
            to.order,
            to.release_count
        ),
        (
            TombPhase::Intent,
            TombOrder::Legacy,
            0,
            TombPhase::DataDeleting,
            TombOrder::BytesThenQuota,
            0
        ) | (
            TombPhase::Intent,
            TombOrder::Legacy,
            0,
            TombPhase::QuotaReleased,
            TombOrder::LegacyReconcile,
            1
        ) | (
            TombPhase::DataDeleting,
            TombOrder::BytesThenQuota,
            0,
            TombPhase::DataAbsent,
            TombOrder::BytesThenQuota,
            0
        ) | (
            TombPhase::DataAbsent,
            TombOrder::BytesThenQuota,
            0,
            TombPhase::QuotaReleased,
            TombOrder::BytesThenQuota,
            1
        ) | (
            TombPhase::QuotaReleased,
            TombOrder::BytesThenQuota,
            1,
            TombPhase::Verified,
            TombOrder::BytesThenQuota,
            1
        ) | (
            TombPhase::QuotaReleased,
            TombOrder::Legacy,
            1,
            TombPhase::QuotaReleased,
            TombOrder::LegacyReconcile,
            1
        ) | (
            TombPhase::QuotaReleased,
            TombOrder::LegacyReconcile,
            1,
            TombPhase::Verified,
            TombOrder::LegacyReconcile,
            1
        ) | (
            TombPhase::QuotaReleased,
            TombOrder::LegacyReconcile,
            1,
            TombPhase::QuotaReleased,
            TombOrder::LegacyReconcile,
            1
        )
    )
}

fn tombstone_pair_ok(phase: TombPhase, release_count: u8, order: TombOrder) -> bool {
    matches!(
        (phase, release_count, order),
        (TombPhase::Intent, 0, TombOrder::Legacy)
            | (TombPhase::QuotaReleased, 1, TombOrder::Legacy)
            | (TombPhase::Verified, 1, TombOrder::Legacy)
            | (TombPhase::DataDeleting, 0, TombOrder::BytesThenQuota)
            | (TombPhase::DataAbsent, 0, TombOrder::BytesThenQuota)
            | (TombPhase::QuotaReleased, 1, TombOrder::BytesThenQuota)
            | (TombPhase::Verified, 1, TombOrder::BytesThenQuota)
            | (TombPhase::QuotaReleased, 1, TombOrder::LegacyReconcile)
            | (TombPhase::Verified, 1, TombOrder::LegacyReconcile)
    )
}

fn open_absence_key(session_id: &str) -> String {
    format!("meta/open-absence/{session_id}")
}

struct OpenAbsence {
    session: String,
    project: String,
    principal: String,
    grant: String,
    object_key: String,
    expected_digest: [u8; 32],
    expected_size: u64,
}

fn encode_open_absence(record: &SessionRecord) -> Result<Vec<u8>, FileHostError> {
    let body = format!(
        "oo.control.local-file-hosting/v1 open-absence\nsession {}\nproject {}\nprincipal {}\ngrant_hex {}\nobject_key {}\nexpected_digest {}\nexpected_size {}\n",
        record.session_id,
        record.project_id,
        record.principal_id,
        hex_encode(record.grant_evidence.as_bytes()),
        record.object_key,
        hex_encode(&record.expected_sha256),
        record.expected_size
    );
    if body.len() > MAX_MANIFEST_BYTES || body.contains('\0') {
        return Err(FileHostError::Corrupt);
    }
    Ok(body.into_bytes())
}

fn decode_open_absence(bytes: &[u8]) -> Result<OpenAbsence, FileHostError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(FileHostError::Corrupt);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| FileHostError::Corrupt)?;
    let mut lines = text.lines();
    let contract = lines.next().ok_or(FileHostError::Corrupt)?;
    if contract != "oo.control.local-file-hosting/v1 open-absence" {
        return Err(FileHostError::Corrupt);
    }
    let session = field_value(&mut lines, "session ")?;
    let project = field_value(&mut lines, "project ")?;
    let principal = field_value(&mut lines, "principal ")?;
    let grant = hex_decode_text(&field_value(&mut lines, "grant_hex ")?)?;
    let object_key = field_value(&mut lines, "object_key ")?;
    let expected_digest = hex_decode_32(&field_value(&mut lines, "expected_digest ")?)
        .ok_or(FileHostError::Corrupt)?;
    let expected_size = parse_u64_token(&field_value(&mut lines, "expected_size ")?)?;
    if lines.next().is_some() || !is_hex(&session, 32) {
        return Err(FileHostError::Corrupt);
    }
    let embedded = session_id_from_object_key(&object_key).ok_or(FileHostError::Corrupt)?;
    let digest_text = object_key.split('/').nth(2).ok_or(FileHostError::Corrupt)?;
    if embedded != session || digest_text != hex_encode(&expected_digest) {
        return Err(FileHostError::Corrupt);
    }
    if ProjectId::new(project.clone()).is_err()
        || PrincipalId::new(principal.clone()).is_err()
        || GrantEvidence::new(grant.clone()).is_err()
    {
        return Err(FileHostError::Corrupt);
    }
    Ok(OpenAbsence {
        session,
        project,
        principal,
        grant,
        object_key,
        expected_digest,
        expected_size,
    })
}

fn canonical_staging_suffix(suffix: &str) -> bool {
    if suffix.is_empty() || (suffix.len() > 1 && suffix.starts_with('0')) {
        return false;
    }
    if !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    matches!(suffix.parse::<u32>(), Ok(value) if (1..=MAX_PART_COUNT).contains(&value))
}

fn is_platform_staging_name(stem: &str, file_name: &str) -> bool {
    let Some((head, suffix)) = file_name.split_once('#') else {
        return false;
    };
    head == stem && canonical_staging_suffix(suffix)
}

fn is_five_digit_part_name(name: &str) -> bool {
    name.len() == 5
        && name.bytes().all(|byte| byte.is_ascii_digit())
        && matches!(name.parse::<u32>(), Ok(value) if (1..=MAX_PART_COUNT).contains(&value))
}

fn is_part_staging_name(name: &str) -> bool {
    let Some((head, suffix)) = name.split_once('#') else {
        return false;
    };
    is_five_digit_part_name(head) && canonical_staging_suffix(suffix)
}

fn owned_parent<'a>(
    root: &std::path::Path,
    key: &'a str,
) -> Result<Option<(std::path::PathBuf, &'a str)>, FileHostError> {
    if key.contains('\0')
        || key
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(FileHostError::Store);
    }
    let mut path = root.to_path_buf();
    let parts: Vec<&str> = key.split('/').collect();
    let Some((stem, parents)) = parts.split_last() else {
        return Err(FileHostError::Store);
    };
    for part in parents {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(FileHostError::Corrupt),
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(FileHostError::Corrupt),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(FileHostError::Store),
        }
    }
    if !path.starts_with(root) {
        return Err(FileHostError::Corrupt);
    }
    Ok(Some((path, stem)))
}

fn fsync_dir(dir: &std::path::Path) -> Result<(), FileHostError> {
    let file = std::fs::File::open(dir).map_err(|_| FileHostError::Store)?;
    file.sync_all().map_err(|_| FileHostError::Store)?;
    Ok(())
}

fn regular_staging_file(path: &std::path::Path) -> Result<bool, FileHostError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(FileHostError::Corrupt),
        Ok(meta) if meta.is_file() => Ok(true),
        Ok(_) => Err(FileHostError::Corrupt),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(FileHostError::Store),
    }
}

fn for_dir_names(
    dir: &std::path::Path,
    mut visit: impl FnMut(&str, &std::path::Path) -> Result<(), FileHostError>,
) -> Result<(), FileHostError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(FileHostError::Store),
    };
    for entry in entries {
        let entry = entry.map_err(|_| FileHostError::Store)?;
        let Some(name) = entry.file_name().into_string().ok() else {
            continue;
        };
        visit(&name, &dir.join(&name))?;
    }
    Ok(())
}

fn refuse_staging_symlink(root: &std::path::Path, key: &str) -> Result<(), FileHostError> {
    let Some((dir, stem)) = owned_parent(root, key)? else {
        return Ok(());
    };
    for_dir_names(&dir, |name, path| {
        if is_platform_staging_name(stem, name) {
            let _present = regular_staging_file(path)?;
        }
        Ok(())
    })
}

fn staging_absent(root: &std::path::Path, key: &str) -> Result<bool, FileHostError> {
    let Some((dir, stem)) = owned_parent(root, key)? else {
        return Ok(true);
    };
    let mut clear = true;
    for_dir_names(&dir, |name, path| {
        if !is_platform_staging_name(stem, name) {
            return Ok(());
        }
        if regular_staging_file(path)? {
            clear = false;
        }
        Ok(())
    })?;
    Ok(clear)
}

fn reap_key_staging(root: &std::path::Path, key: &str) -> Result<(), FileHostError> {
    let Some((dir, stem)) = owned_parent(root, key)? else {
        return Ok(());
    };
    let mut removed = false;
    for_dir_names(&dir, |name, path| {
        if !is_platform_staging_name(stem, name) {
            return Ok(());
        }
        if !path.starts_with(root) {
            return Err(FileHostError::Corrupt);
        }
        if !regular_staging_file(path)? {
            return Ok(());
        }
        match std::fs::remove_file(path) {
            Ok(()) => removed = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(FileHostError::Store),
        }
        Ok(())
    })?;
    if removed {
        fsync_dir(&dir)?;
    }
    Ok(())
}

fn part_directory(
    root: &std::path::Path,
    session_id: &str,
) -> Result<Option<std::path::PathBuf>, FileHostError> {
    let key = format!("staging/{session_id}/part/00001");
    Ok(owned_parent(root, &key)?.map(|(dir, _)| dir))
}

fn refuse_part_symlinks(root: &std::path::Path, session_id: &str) -> Result<(), FileHostError> {
    let Some(dir) = part_directory(root, session_id)? else {
        return Ok(());
    };
    for_dir_names(&dir, |name, path| {
        if is_five_digit_part_name(name) || is_part_staging_name(name) {
            let _present = regular_staging_file(path)?;
        }
        Ok(())
    })
}

fn part_dir_clear(root: &std::path::Path, session_id: &str) -> Result<bool, FileHostError> {
    let Some(dir) = part_directory(root, session_id)? else {
        return Ok(true);
    };
    let mut clear = true;
    for_dir_names(&dir, |name, path| {
        if (is_five_digit_part_name(name) || is_part_staging_name(name))
            && regular_staging_file(path)?
        {
            clear = false;
        }
        Ok(())
    })?;
    Ok(clear)
}

fn reap_part_staging(root: &std::path::Path, session_id: &str) -> Result<(), FileHostError> {
    let Some(dir) = part_directory(root, session_id)? else {
        return Ok(());
    };
    let mut removed = false;
    for_dir_names(&dir, |name, path| {
        if !is_part_staging_name(name) {
            return Ok(());
        }
        if !path.starts_with(root) {
            return Err(FileHostError::Corrupt);
        }
        if !regular_staging_file(path)? {
            return Ok(());
        }
        match std::fs::remove_file(path) {
            Ok(()) => removed = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(FileHostError::Store),
        }
        Ok(())
    })?;
    if removed {
        fsync_dir(&dir)?;
    }
    Ok(())
}

fn encode_tombstone(tomb: &Tombstone) -> Result<Vec<u8>, FileHostError> {
    if tomb.quota_bytes != tomb.manifest.size || tomb.quota_bytes == 0 {
        return Err(FileHostError::Corrupt);
    }
    let phase = match (tomb.phase, tomb.release_count, tomb.order) {
        (TombPhase::DataDeleting, 0, TombOrder::BytesThenQuota) => "data_deleting",
        (TombPhase::DataAbsent, 0, TombOrder::BytesThenQuota) => "data_absent",
        (TombPhase::QuotaReleased, 1, TombOrder::BytesThenQuota) => "quota_released",
        (TombPhase::Verified, 1, TombOrder::BytesThenQuota) => "verified",
        (TombPhase::QuotaReleased, 1, TombOrder::LegacyReconcile) => "quota_released",
        (TombPhase::Verified, 1, TombOrder::LegacyReconcile) => "verified",
        _ => return Err(FileHostError::Corrupt),
    };
    let order = match tomb.order {
        TombOrder::BytesThenQuota => "bytes_then_quota",
        TombOrder::LegacyReconcile => "legacy_reconcile",
        TombOrder::Legacy => return Err(FileHostError::Corrupt),
    };
    let content_type = tomb.manifest.content_type.as_deref().unwrap_or("-");
    let body = format!(
        "oo.control.local-file-hosting/v1 tombstone\nphase {phase}\nobject_key {}\nobject_digest {}\nsize {}\nlineage_digest {}\nproject {}\nprincipal {}\ngrant_hex {}\ndisplay_name {}\ncontent_type {content_type}\npart_count {}\nidempotency_key {}\nquota_key {}\nquota_bytes {}\nrelease_count {}\norder {order}\n",
        tomb.manifest.object_key.as_str(),
        hex_encode(&tomb.manifest.object_digest),
        tomb.manifest.size,
        hex_encode(&tomb.manifest.lineage_digest),
        tomb.manifest.project_id.as_str(),
        tomb.manifest.principal_id.as_str(),
        hex_encode(tomb.manifest.grant_evidence.as_str().as_bytes()),
        tomb.manifest.display_name,
        tomb.manifest.part_count,
        tomb.manifest.idempotency_key.as_str(),
        tomb.quota_key,
        tomb.quota_bytes,
        tomb.release_count
    );
    if body.len() > MAX_MANIFEST_BYTES || body.contains('\0') {
        return Err(FileHostError::Corrupt);
    }
    Ok(body.into_bytes())
}

fn decode_tombstone(bytes: &[u8]) -> Result<Tombstone, FileHostError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(FileHostError::Corrupt);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| FileHostError::Corrupt)?;
    let mut lines = text.lines();
    let contract = lines.next().ok_or(FileHostError::Corrupt)?;
    if contract != "oo.control.local-file-hosting/v1 tombstone" {
        return Err(FileHostError::Corrupt);
    }
    let phase_text = field_value(&mut lines, "phase ")?;
    let object_key = field_value(&mut lines, "object_key ")?;
    let object_digest =
        hex_decode_32(&field_value(&mut lines, "object_digest ")?).ok_or(FileHostError::Corrupt)?;
    let size = parse_u64_token(&field_value(&mut lines, "size ")?)?;
    let lineage_digest = hex_decode_32(&field_value(&mut lines, "lineage_digest ")?)
        .ok_or(FileHostError::Corrupt)?;
    let project = field_value(&mut lines, "project ")?;
    let principal = field_value(&mut lines, "principal ")?;
    let grant = hex_decode_text(&field_value(&mut lines, "grant_hex ")?)?;
    let display_name = field_value(&mut lines, "display_name ")?;
    let content_type = field_value(&mut lines, "content_type ")?;
    let part_count = parse_u64_token(&field_value(&mut lines, "part_count ")?)?;
    let idempotency = field_value(&mut lines, "idempotency_key ")?;
    let quota_key = field_value(&mut lines, "quota_key ")?;
    let quota_bytes = parse_u64_token(&field_value(&mut lines, "quota_bytes ")?)?;
    let release_count = parse_u64_token(&field_value(&mut lines, "release_count ")?)?;
    let order = match lines.next() {
        None => TombOrder::Legacy,
        Some("order bytes_then_quota") => TombOrder::BytesThenQuota,
        Some("order legacy_reconcile") => TombOrder::LegacyReconcile,
        Some(_) => return Err(FileHostError::Corrupt),
    };
    if lines.next().is_some() || size == 0 || size != quota_bytes || release_count > 1 {
        return Err(FileHostError::Corrupt);
    }
    let release_count = u8::try_from(release_count).map_err(|_| FileHostError::Corrupt)?;
    let phase = match phase_text.as_str() {
        "intent" => TombPhase::Intent,
        "data_deleting" => TombPhase::DataDeleting,
        "data_absent" => TombPhase::DataAbsent,
        "quota_released" => TombPhase::QuotaReleased,
        "verified" => TombPhase::Verified,
        _ => return Err(FileHostError::Corrupt),
    };
    if !tombstone_pair_ok(phase, release_count, order) {
        return Err(FileHostError::Corrupt);
    }
    let session_id = session_id_from_object_key(&object_key).ok_or(FileHostError::Corrupt)?;
    let embedded = object_key.split('/').nth(2).ok_or(FileHostError::Corrupt)?;
    if embedded != hex_encode(&object_digest) {
        return Err(FileHostError::Corrupt);
    }
    let expected_quota = quota_object_key_str(&project, &session_id);
    if quota_key != expected_quota
        || !is_hex(&idempotency, 64)
        || !valid_display_name(&display_name)
    {
        return Err(FileHostError::Corrupt);
    }
    let content_type = if content_type == "-" {
        None
    } else if valid_content_type(&content_type) {
        Some(content_type)
    } else {
        return Err(FileHostError::Corrupt);
    };
    let part_count = u32::try_from(part_count).map_err(|_| FileHostError::Corrupt)?;
    if part_count == 0 || part_count > MAX_PART_COUNT {
        return Err(FileHostError::Corrupt);
    }
    let manifest = ObjectManifest {
        contract: CONTRACT,
        object_key: ObjectKey::parse(object_key).map_err(|_| FileHostError::Corrupt)?,
        session_id: SessionId::parse(session_id).map_err(|_| FileHostError::Corrupt)?,
        project_id: ProjectId::new(project).map_err(|_| FileHostError::Corrupt)?,
        principal_id: PrincipalId::new(principal).map_err(|_| FileHostError::Corrupt)?,
        grant_evidence: GrantEvidence::new(grant).map_err(|_| FileHostError::Corrupt)?,
        display_name,
        content_type,
        object_digest,
        size,
        part_count,
        idempotency_key: IdempotencyKey(idempotency),
        lineage_digest,
    };
    Ok(Tombstone {
        phase,
        manifest,
        quota_key,
        quota_bytes,
        release_count,
        order,
    })
}

fn field_value<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    prefix: &str,
) -> Result<String, FileHostError> {
    let line = lines.next().ok_or(FileHostError::Corrupt)?;
    let value = line.strip_prefix(prefix).ok_or(FileHostError::Corrupt)?;
    if value.is_empty() {
        Err(FileHostError::Corrupt)
    } else {
        Ok(value.to_owned())
    }
}

fn parse_u64_token(value: &str) -> Result<u64, FileHostError> {
    if value.is_empty() || (value.len() > 1 && value.starts_with('0')) {
        return Err(FileHostError::Corrupt);
    }
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FileHostError::Corrupt);
    }
    value.parse().map_err(|_| FileHostError::Corrupt)
}

fn hex_decode_text(value: &str) -> Result<String, FileHostError> {
    if !value.len().is_multiple_of(2) || value.len() > MAX_GRANT_BYTES * 2 {
        return Err(FileHostError::Corrupt);
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let raw = value.as_bytes();
    for index in (0..raw.len()).step_by(2) {
        let hi = hex_nibble(raw[index]).ok_or(FileHostError::Corrupt)?;
        let lo = hex_nibble(raw[index + 1]).ok_or(FileHostError::Corrupt)?;
        bytes.push((hi << 4) | lo);
    }
    String::from_utf8(bytes).map_err(|_| FileHostError::Corrupt)
}

/// One bounded pull of a published object.
///
/// The manifest snapshot is fixed at open. [`ObjectDownload::is_complete`] becomes
/// true only after the bytes returned to the caller and a separate bounded rehash
/// of the stored object both match that snapshot, and the manifest bytes and
/// stored length still match before and after that rehash. On a local deployment
/// the file's device, inode, modified time, and length must also stay the same
/// across the rehash. Stopping early, cancelling, passing the deadline, or
/// dropping the reader does not complete the pull. A write that lands after
/// completion does not clear it: the check is that clean end, not a later
/// revocation. There is no download credential and no [`std::io::Read`] cancel.
pub struct ObjectDownload {
    store: Arc<dyn ObjectStore>,
    location: Path,
    manifest_key: String,
    manifest: ObjectManifest,
    manifest_bytes: Vec<u8>,
    /// Regular file under the local deployment root. Empty for [`FileHost::new`].
    local_file: Option<std::path::PathBuf>,
    chunk_budget: usize,
    deadline: Option<std::time::Instant>,
    cancel: Arc<AtomicBool>,
    offset: u64,
    hasher: Sha256,
    complete: bool,
    stop: Option<FileHostError>,
}

impl std::fmt::Debug for ObjectDownload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectDownload")
            .field("object_key", &self.manifest.object_key)
            .field("size", &self.manifest.size)
            .field("offset", &self.offset)
            .field("chunk_budget", &self.chunk_budget)
            .field("complete", &self.complete)
            .finish_non_exhaustive()
    }
}

impl Drop for ObjectDownload {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

impl ObjectDownload {
    /// Manifest captured when the pull opened. Later store edits do not replace it.
    pub fn manifest(&self) -> &ObjectManifest {
        &self.manifest
    }

    /// `true` only after a verified end. A dropped or partial pull stays `false`.
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Refuse later chunks. This does not grant a credential and does not cancel
    /// a filesystem read that has already started.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Next range, or `Ok(None)` after the size and sha256 have been verified.
    ///
    /// The vector is at most the chunk budget. The reader does not keep it.
    /// `Ok(Some)` is not completion. A cancel or a passed deadline is
    /// [`FileHostError::Stopped`] and leaves the pull incomplete.
    pub async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, FileHostError> {
        let mut arm = CancelArm {
            cancel: Arc::clone(&self.cancel),
            armed: true,
        };
        let result = self.pull_chunk().await;
        arm.armed = false;
        result
    }

    async fn pull_chunk(&mut self) -> Result<Option<Vec<u8>>, FileHostError> {
        if self.complete {
            return Ok(None);
        }
        self.ensure_running()?;
        if self.offset == self.manifest.size {
            self.finish_eof().await?;
            return Ok(None);
        }
        if self.offset > self.manifest.size {
            return Err(self.fail(FileHostError::Corrupt));
        }
        let chunk = self
            .read_download_range(self.offset, self.manifest.size, self.chunk_budget)
            .await?;
        let Ok(len) = u64::try_from(chunk.len()) else {
            return Err(self.fail(FileHostError::Corrupt));
        };
        let Some(next) = self.offset.checked_add(len) else {
            return Err(self.fail(FileHostError::Corrupt));
        };
        if next > self.manifest.size {
            return Err(self.fail(FileHostError::Corrupt));
        }
        self.hasher.update(&chunk);
        self.offset = next;
        Ok(Some(chunk))
    }

    async fn finish_eof(&mut self) -> Result<(), FileHostError> {
        self.ensure_running()?;
        let manifest_before = self.reload_manifest_bytes().await?;
        if manifest_before != self.manifest_bytes {
            return Err(self.fail(FileHostError::Corrupt));
        }
        let object_key = self.manifest.object_key.as_str().to_owned();
        let head_before = self.await_head(&object_key).await?;
        if head_before.len != self.manifest.size || self.offset != self.manifest.size {
            return Err(self.fail(FileHostError::Corrupt));
        }
        let local_before = self.stat_pinned()?;
        if local_before.is_some_and(|stat| stat.len != self.manifest.size) {
            return Err(self.fail(FileHostError::Corrupt));
        }
        let stored = self.rehash_stored().await?;
        let manifest_after = self.reload_manifest_bytes().await?;
        let head_after = self.await_head(&object_key).await?;
        let local_after = self.stat_pinned()?;
        self.ensure_running()?;
        if manifest_after != self.manifest_bytes
            || head_after != head_before
            || local_after != local_before
        {
            return Err(self.fail(FileHostError::Corrupt));
        }
        let returned = finalize(self.hasher.clone());
        if returned != self.manifest.object_digest || stored != self.manifest.object_digest {
            return Err(self.fail(FileHostError::Corrupt));
        }
        self.complete = true;
        Ok(())
    }

    /// Re-read the stored object in ranges of at most 8192 bytes.
    ///
    /// The returned vector from each range is dropped before the next range starts.
    async fn rehash_stored(&mut self) -> Result<[u8; 32], FileHostError> {
        let known = self.manifest.size;
        let mut hasher = Sha256::new();
        let mut offset = 0u64;
        while offset < known {
            let chunk = self
                .read_download_range(offset, known, STREAM_CHUNK_BYTES)
                .await?;
            let len = match u64::try_from(chunk.len()) {
                Ok(len) => len,
                Err(_) => return Err(self.fail(FileHostError::Corrupt)),
            };
            hasher.update(&chunk);
            offset = match offset.checked_add(len) {
                Some(next) => next,
                None => return Err(self.fail(FileHostError::Corrupt)),
            };
        }
        if offset != known {
            return Err(self.fail(FileHostError::Corrupt));
        }
        Ok(finalize(hasher))
    }

    async fn read_download_range(
        &mut self,
        offset: u64,
        known: u64,
        chunk_limit: usize,
    ) -> Result<Vec<u8>, FileHostError> {
        if chunk_limit == 0 || chunk_limit > STREAM_CHUNK_BYTES {
            return Err(self.fail(FileHostError::Store));
        }
        let step = match u64::try_from(chunk_limit) {
            Ok(step) => step,
            Err(_) => return Err(self.fail(FileHostError::Corrupt)),
        };
        if offset >= known {
            return Err(self.fail(FileHostError::Store));
        }
        let end = offset.saturating_add(step).min(known);
        let options = GetOptions {
            range: Some(GetRange::Bounded(offset..end)),
            ..GetOptions::default()
        };
        let location = self.location.clone();
        let got = self.await_get(&location, options).await?;
        let chunk = self.await_bytes(got).await?;
        if chunk.is_empty() || chunk.len() > chunk_limit {
            return Err(self.fail(FileHostError::Store));
        }
        let expected = match usize::try_from(end - offset) {
            Ok(expected) => expected,
            Err(_) => return Err(self.fail(FileHostError::Corrupt)),
        };
        if chunk.len() != expected {
            return Err(self.fail(FileHostError::Store));
        }
        Ok(chunk)
    }

    async fn reload_manifest_bytes(&mut self) -> Result<Vec<u8>, FileHostError> {
        let key = self.manifest_key.clone();
        let head = self.await_head(&key).await?;
        let cap = match u64::try_from(MAX_MANIFEST_BYTES) {
            Ok(cap) => cap,
            Err(_) => return Err(self.fail(FileHostError::Corrupt)),
        };
        if head.len == 0 || head.len > cap {
            return Err(self.fail(FileHostError::Corrupt));
        }
        let path = parse_store_key(&key)?;
        let options = GetOptions {
            range: Some(GetRange::Bounded(0..head.len)),
            ..GetOptions::default()
        };
        let got = self.await_get(&path, options).await?;
        let bytes = self.await_bytes(got).await?;
        if u64::try_from(bytes.len()).ok() != Some(head.len) || bytes.len() > MAX_MANIFEST_BYTES {
            return Err(self.fail(FileHostError::Store));
        }
        Ok(bytes)
    }

    async fn await_head(&mut self, key: &str) -> Result<HeadSeen, FileHostError> {
        self.ensure_running()?;
        let path = parse_store_key(key)?;
        let store = Arc::clone(&self.store);
        let pending = store.head(&path).await;
        self.ensure_running()?;
        match pending {
            Ok(meta) => Ok(HeadSeen {
                len: meta.size,
                e_tag: meta.e_tag,
                version: meta.version,
            }),
            Err(object_store::Error::NotFound { .. }) => Err(self.fail(FileHostError::Corrupt)),
            Err(_) => Err(self.fail(FileHostError::Store)),
        }
    }

    async fn await_get(
        &mut self,
        path: &Path,
        options: GetOptions,
    ) -> Result<GetResult, FileHostError> {
        self.ensure_running()?;
        let store = Arc::clone(&self.store);
        let path = path.clone();
        let pending = store.get_opts(&path, options).await;
        self.ensure_running()?;
        match pending {
            Ok(got) => Ok(got),
            Err(_) => Err(self.fail(FileHostError::Store)),
        }
    }

    async fn await_bytes(&mut self, got: GetResult) -> Result<Vec<u8>, FileHostError> {
        self.ensure_running()?;
        let pending = got.bytes().await;
        self.ensure_running()?;
        match pending {
            Ok(chunk) => Ok(chunk.to_vec()),
            Err(_) => Err(self.fail(FileHostError::Store)),
        }
    }

    /// Stat the local object when this reader was opened on a deployment directory.
    ///
    /// The stat is not cancelled if it has started. The deadline is checked
    /// before the call and again after it returns, and a passed deadline does
    /// not start the next await. The local backend reports an empty object
    /// version; that field is not this identity.
    fn stat_pinned(&mut self) -> Result<Option<LocalStat>, FileHostError> {
        let Some(path) = self.local_file.clone() else {
            return Ok(None);
        };
        self.ensure_running()?;
        let stat = match stat_local(&path) {
            Ok(stat) => stat,
            Err(error) => return Err(self.fail(error)),
        };
        self.ensure_running()?;
        Ok(Some(stat))
    }

    fn ensure_running(&mut self) -> Result<(), FileHostError> {
        if let Some(error) = self.stop {
            return Err(error);
        }
        if self.cancel.load(Ordering::SeqCst) {
            return Err(self.fail(FileHostError::Stopped));
        }
        if self
            .deadline
            .is_some_and(|deadline| std::time::Instant::now() >= deadline)
        {
            return Err(self.fail(FileHostError::Stopped));
        }
        Ok(())
    }

    fn fail(&mut self, error: FileHostError) -> FileHostError {
        self.complete = false;
        *self.stop.get_or_insert(error)
    }
}

struct CancelArm {
    cancel: Arc<AtomicBool>,
    armed: bool,
}

impl Drop for CancelArm {
    fn drop(&mut self) {
        if self.armed {
            self.cancel.store(true, Ordering::SeqCst);
        }
    }
}

struct Gate {
    owner: AtomicU64,
}

struct HeldLock {
    file: std::fs::File,
    gate: Arc<Gate>,
    token: u64,
}

impl Drop for HeldLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
        release_gate(&self.gate, self.token);
    }
}

struct LockWait {
    cancel: AtomicBool,
    slot: Mutex<Option<Result<HeldLock, FileHostError>>>,
    waker: Mutex<Option<Waker>>,
}

struct CancelOnDrop {
    wait: Arc<LockWait>,
    armed: bool,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.wait.cancel.store(true, Ordering::SeqCst);
        if let Some(Ok(held)) = lock_mutex(&self.wait.slot).take() {
            drop(held);
        }
    }
}

fn lock_mutex<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn gates() -> &'static Mutex<BTreeMap<std::path::PathBuf, Arc<Gate>>> {
    static GATES: OnceLock<Mutex<BTreeMap<std::path::PathBuf, Arc<Gate>>>> = OnceLock::new();
    GATES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn gate_for(path: &std::path::Path) -> Result<Arc<Gate>, FileHostError> {
    let mut map = lock_mutex(gates());
    Ok(map
        .entry(path.to_path_buf())
        .or_insert_with(|| {
            Arc::new(Gate {
                owner: AtomicU64::new(0),
            })
        })
        .clone())
}

fn next_lock_token() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    loop {
        let token = NEXT.fetch_add(1, Ordering::Relaxed);
        if token != 0 {
            return token;
        }
    }
}

fn release_gate(gate: &Gate, token: u64) {
    let _ = gate
        .owner
        .compare_exchange(token, 0, Ordering::SeqCst, Ordering::SeqCst);
}

fn deployment_lock_path(root: &std::path::Path) -> Result<std::path::PathBuf, FileHostError> {
    let canonical = std::fs::canonicalize(root).map_err(|_| FileHostError::Store)?;
    let mut name = canonical
        .file_name()
        .ok_or(FileHostError::Store)?
        .to_os_string();
    name.push(".oo-file-hosting.lock");
    let parent = canonical.parent().ok_or(FileHostError::Store)?;
    Ok(parent.join(name))
}

fn blocking_lock(path: std::path::PathBuf, gate: Arc<Gate>, token: u64, wait: Arc<LockWait>) {
    let outcome = acquire_lock(&path, &gate, token, &wait);
    {
        let mut slot = lock_mutex(&wait.slot);
        if wait.cancel.load(Ordering::SeqCst) {
            drop(outcome.ok());
            *slot = Some(Err(FileHostError::Unavailable));
        } else {
            *slot = Some(outcome);
        }
    }
    let waker = lock_mutex(&wait.waker).take();
    if let Some(waker) = waker {
        waker.wake();
    }
}

fn acquire_lock(
    path: &std::path::Path,
    gate: &Arc<Gate>,
    token: u64,
    wait: &LockWait,
) -> Result<HeldLock, FileHostError> {
    let started = Instant::now();
    loop {
        if wait.cancel.load(Ordering::SeqCst) {
            return Err(FileHostError::Unavailable);
        }
        if gate
            .owner
            .compare_exchange(0, token, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            break;
        }
        if started.elapsed() >= LOCK_WAIT {
            return Err(FileHostError::Unavailable);
        }
        std::thread::sleep(LOCK_POLL);
    }
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
    {
        Ok(file) => file,
        Err(_) => {
            release_gate(gate, token);
            return Err(FileHostError::Unavailable);
        }
    };
    loop {
        if wait.cancel.load(Ordering::SeqCst) {
            release_gate(gate, token);
            return Err(FileHostError::Unavailable);
        }
        match file.try_lock() {
            Ok(()) => {
                return Ok(HeldLock {
                    file,
                    gate: Arc::clone(gate),
                    token,
                });
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                if started.elapsed() >= LOCK_WAIT {
                    release_gate(gate, token);
                    return Err(FileHostError::Unavailable);
                }
                std::thread::sleep(LOCK_POLL);
            }
            Err(_) => {
                release_gate(gate, token);
                return Err(FileHostError::Unavailable);
            }
        }
    }
}

async fn store_len(store: &dyn ObjectStore, key: &str) -> Result<Option<u64>, FileHostError> {
    let path = parse_store_key(key)?;
    match store.head(&path).await {
        Ok(meta) => Ok(Some(meta.size)),
        Err(object_store::Error::NotFound { .. }) => Ok(None),
        Err(_) => Err(FileHostError::Store),
    }
}

async fn load_capped_bytes(
    store: &dyn ObjectStore,
    key: &str,
    cap: usize,
) -> Result<Option<Vec<u8>>, FileHostError> {
    let Some(len) = store_len(store, key).await? else {
        return Ok(None);
    };
    let cap_len = u64::try_from(cap).map_err(|_| FileHostError::Corrupt)?;
    if len == 0 || len > cap_len {
        return Err(FileHostError::Corrupt);
    }
    let path = parse_store_key(key)?;
    let options = GetOptions {
        range: Some(GetRange::Bounded(0..len)),
        ..GetOptions::default()
    };
    let got = store
        .get_opts(&path, options)
        .await
        .map_err(|_| FileHostError::Store)?;
    let bytes = got.bytes().await.map_err(|_| FileHostError::Store)?;
    if u64::try_from(bytes.len()).ok() != Some(len) || bytes.len() > cap {
        return Err(FileHostError::Store);
    }
    Ok(Some(bytes.to_vec()))
}

async fn store_bytes(store: &dyn ObjectStore, key: &str) -> Result<Option<Vec<u8>>, FileHostError> {
    let path = parse_store_key(key)?;
    match store.get(&path).await {
        Ok(got) => {
            let bytes = got.bytes().await.map_err(|_| FileHostError::Store)?;
            Ok(Some(bytes.to_vec()))
        }
        Err(object_store::Error::NotFound { .. }) => Ok(None),
        Err(_) => Err(FileHostError::Store),
    }
}

#[derive(Clone, PartialEq, Eq)]
struct HeadSeen {
    len: u64,
    e_tag: Option<String>,
    version: Option<String>,
}

/// Device, inode, modified time, and length of one local object file.
///
/// This is the identity a safe stat can actually observe. The local backend's
/// object `version` is empty and is not a substitute for these fields.
#[derive(Clone, Copy, PartialEq, Eq)]
struct LocalStat {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: std::time::SystemTime,
}

fn local_object_file(
    root: &std::path::Path,
    key: &str,
) -> Result<std::path::PathBuf, FileHostError> {
    if key.contains('\0')
        || key
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(FileHostError::Store);
    }
    let root = std::fs::canonicalize(root).map_err(|_| FileHostError::Store)?;
    let path = root.join(key);
    let parent = path.parent().ok_or(FileHostError::Store)?;
    let parent = std::fs::canonicalize(parent).map_err(|_| FileHostError::Store)?;
    if !parent.starts_with(&root) {
        return Err(FileHostError::Store);
    }
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_file() => Ok(path),
        Ok(_) => Err(FileHostError::Corrupt),
        Err(_) => Err(FileHostError::Store),
    }
}

fn stat_local(path: &std::path::Path) -> Result<LocalStat, FileHostError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(FileHostError::Corrupt);
        }
        Err(_) => return Err(FileHostError::Store),
    };
    if !meta.file_type().is_file() {
        return Err(FileHostError::Corrupt);
    }
    let mtime = meta.modified().map_err(|_| FileHostError::Store)?;
    #[cfg(unix)]
    let (dev, ino) = {
        use std::os::unix::fs::MetadataExt;
        (meta.dev(), meta.ino())
    };
    #[cfg(not(unix))]
    let (dev, ino) = (0u64, 0u64);
    Ok(LocalStat {
        dev,
        ino,
        len: meta.len(),
        mtime,
    })
}

async fn read_bounded_range(
    store: &dyn ObjectStore,
    path: &Path,
    offset: u64,
    known: u64,
    chunk_limit: usize,
) -> Result<Vec<u8>, FileHostError> {
    if chunk_limit == 0 || chunk_limit > STREAM_CHUNK_BYTES {
        return Err(FileHostError::Store);
    }
    let step = u64::try_from(chunk_limit).map_err(|_| FileHostError::Corrupt)?;
    if offset >= known {
        return Err(FileHostError::Store);
    }
    let end = offset.saturating_add(step).min(known);
    let options = GetOptions {
        range: Some(GetRange::Bounded(offset..end)),
        ..GetOptions::default()
    };
    let got = store
        .get_opts(path, options)
        .await
        .map_err(|_| FileHostError::Store)?;
    let chunk = got.bytes().await.map_err(|_| FileHostError::Store)?;
    if chunk.is_empty() || chunk.len() > chunk_limit {
        return Err(FileHostError::Store);
    }
    let expected = usize::try_from(end - offset).map_err(|_| FileHostError::Corrupt)?;
    if chunk.len() != expected {
        return Err(FileHostError::Store);
    }
    Ok(chunk.to_vec())
}

struct ValidatedConfig {
    display_name: String,
    expected_sha256: [u8; 32],
    expected_size: u64,
    content_type: Option<String>,
}

/// Deterministic idempotency key for this context and configuration.
///
/// The key binds the project, the principal, the expected object sha256, the
/// expected size, the display name, and the content type. Grant evidence is
/// not part of the key. A different configuration is a different key.
pub fn idempotency_key(
    ctx: &AdmissionContext,
    config: &UploadConfig,
) -> Result<IdempotencyKey, FileHostError> {
    let validated = validate_config_shape(config)?;
    Ok(IdempotencyKey(hex_encode(&idempotency_digest(
        ctx, &validated,
    ))))
}

fn validate_config(
    config: &UploadConfig,
    limits: HostLimits,
) -> Result<ValidatedConfig, FileHostError> {
    let validated = validate_config_shape(config)?;
    if validated.expected_size > limits.max_object_bytes {
        return Err(FileHostError::Limit);
    }
    Ok(validated)
}

fn validate_config_shape(config: &UploadConfig) -> Result<ValidatedConfig, FileHostError> {
    if config.expected_size == 0 || config.expected_size > MAX_OBJECT_BYTES {
        return Err(FileHostError::Limit);
    }
    if !valid_display_name(&config.display_name) {
        return Err(FileHostError::Admission);
    }
    if let Some(content_type) = &config.content_type
        && !valid_content_type(content_type)
    {
        return Err(FileHostError::Admission);
    }
    Ok(ValidatedConfig {
        display_name: config.display_name.clone(),
        expected_sha256: config.expected_sha256,
        expected_size: config.expected_size,
        content_type: config.content_type.clone(),
    })
}

fn idempotency_digest(ctx: &AdmissionContext, config: &ValidatedConfig) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(IDEM_DOMAIN);
    feed(&mut hasher, ctx.project_id.as_str());
    feed(&mut hasher, ctx.principal_id.as_str());
    feed(&mut hasher, &hex_encode(&config.expected_sha256));
    feed(&mut hasher, &config.expected_size.to_string());
    feed(&mut hasher, &config.display_name);
    feed(&mut hasher, config.content_type.as_deref().unwrap_or(""));
    finalize(hasher)
}

fn feed(hasher: &mut Sha256, value: &str) {
    hasher.update(value.as_bytes());
    hasher.update([0]);
}

fn finalize(hasher: Sha256) -> [u8; 32] {
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    finalize(hasher)
}

fn lineage_digest(
    record: &SessionRecord,
    object_digest: [u8; 32],
    size: u64,
    part_count: u32,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(LINEAGE_DOMAIN);
    feed(&mut hasher, &record.session_id);
    feed(&mut hasher, &record.project_id);
    feed(&mut hasher, &record.principal_id);
    feed(&mut hasher, &record.grant_evidence);
    feed(&mut hasher, &record.object_key);
    feed(&mut hasher, &hex_encode(&object_digest));
    feed(&mut hasher, &size.to_string());
    feed(&mut hasher, &part_count.to_string());
    feed(&mut hasher, &record.idempotency_key);
    feed(&mut hasher, &record.display_name);
    feed(&mut hasher, record.content_type.as_deref().unwrap_or(""));
    finalize(hasher)
}

fn binding_matches(
    record: &SessionRecord,
    ctx: &AdmissionContext,
    config: &ValidatedConfig,
    idempotency_key: &str,
) -> bool {
    record.project_id == ctx.project_id.as_str()
        && record.principal_id == ctx.principal_id.as_str()
        && record.expected_sha256 == config.expected_sha256
        && record.expected_size == config.expected_size
        && record.display_name == config.display_name
        && record.content_type == config.content_type
        && record.idempotency_key == idempotency_key
        && record.object_key == object_key_for(&record.session_id, &config.expected_sha256)
}

fn authorize_record(record: &SessionRecord, ctx: &AdmissionContext) -> Result<(), FileHostError> {
    if record.project_id == ctx.project_id.as_str()
        && record.principal_id == ctx.principal_id.as_str()
    {
        Ok(())
    } else {
        Err(FileHostError::Denied)
    }
}

fn authorize_manifest(
    manifest: &ObjectManifest,
    ctx: &AdmissionContext,
) -> Result<(), FileHostError> {
    if manifest.project_id.as_str() == ctx.project_id.as_str()
        && manifest.principal_id.as_str() == ctx.principal_id.as_str()
    {
        Ok(())
    } else {
        Err(FileHostError::Denied)
    }
}

fn session_view(record: &SessionRecord) -> UploadSession {
    UploadSession {
        session_id: SessionId(record.session_id.clone()),
        object_key: ObjectKey(record.object_key.clone()),
        idempotency_key: IdempotencyKey(record.idempotency_key.clone()),
        expected_sha256: record.expected_sha256,
        expected_size: record.expected_size,
    }
}

fn manifest_from_record(
    record: &SessionRecord,
    object_digest: [u8; 32],
    size: u64,
    part_count: u32,
) -> ObjectManifest {
    let lineage = lineage_digest(record, object_digest, size, part_count);
    ObjectManifest {
        contract: CONTRACT,
        object_key: ObjectKey(record.object_key.clone()),
        session_id: SessionId(record.session_id.clone()),
        project_id: ProjectId(record.project_id.clone()),
        principal_id: PrincipalId(record.principal_id.clone()),
        grant_evidence: GrantEvidence(record.grant_evidence.clone()),
        display_name: record.display_name.clone(),
        content_type: record.content_type.clone(),
        object_digest,
        size,
        part_count,
        idempotency_key: IdempotencyKey(record.idempotency_key.clone()),
        lineage_digest: lineage,
    }
}

fn parts_are_contiguous(parts: &BTreeMap<u32, u64>) -> bool {
    if parts.is_empty() {
        return false;
    }
    let mut expected = 1u32;
    for number in parts.keys() {
        if *number != expected {
            return false;
        }
        expected = match expected.checked_add(1) {
            Some(next) => next,
            None => return false,
        };
    }
    true
}

fn object_key_for(session_id: &str, digest: &[u8; 32]) -> String {
    format!("uploads/{session_id}/{}/original", hex_encode(digest))
}

fn session_object_key(session_id: &str) -> String {
    format!("meta/session/{session_id}")
}

fn manifest_object_key(session_id: &SessionId) -> String {
    manifest_object_key_str(session_id.as_str())
}

fn manifest_object_key_str(session_id: &str) -> String {
    format!("meta/manifest/{session_id}")
}

fn quota_object_key(ctx: &AdmissionContext, session_id: &str) -> String {
    quota_object_key_str(ctx.project_id.as_str(), session_id)
}

fn quota_object_key_str(project_id: &str, session_id: &str) -> String {
    format!("meta/quota/{project_id}/{session_id}")
}

fn idempotency_pointer_key(
    ctx: &AdmissionContext,
    idempotency_key: &str,
) -> Result<String, FileHostError> {
    idempotency_pointer_key_str(
        ctx.project_id.as_str(),
        ctx.principal_id.as_str(),
        idempotency_key,
    )
}

fn idempotency_pointer_key_str(
    project_id: &str,
    principal_id: &str,
    idempotency_key: &str,
) -> Result<String, FileHostError> {
    if !is_hex(idempotency_key, 64) {
        return Err(FileHostError::Corrupt);
    }
    Ok(format!(
        "meta/idempotency/{project_id}/{principal_id}/{idempotency_key}"
    ))
}

fn part_object_key(session_id: &SessionId, part_number: u32) -> Result<String, FileHostError> {
    part_object_key_str(session_id.as_str(), part_number)
}

fn part_object_key_str(session_id: &str, part_number: u32) -> Result<String, FileHostError> {
    if part_number == 0 || part_number > MAX_PART_COUNT {
        return Err(FileHostError::Limit);
    }
    Ok(format!("staging/{session_id}/part/{part_number:05}"))
}

fn part_number_from_key(location: &str, session_id: &str) -> Option<u32> {
    let prefix = format!("staging/{session_id}/part/");
    let name = location.strip_prefix(&prefix)?;
    if name.len() != 5 || !name.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = name.parse().ok()?;
    if number == 0 || number > MAX_PART_COUNT {
        None
    } else {
        Some(number)
    }
}

fn session_id_from_object_key(object_key: &str) -> Option<String> {
    let mut parts = object_key.split('/');
    if parts.next()? != "uploads" {
        return None;
    }
    let session_id = parts.next()?;
    let digest = parts.next()?;
    let name = parts.next()?;
    if parts.next().is_some()
        || name != "original"
        || !is_hex(session_id, 32)
        || !is_hex(digest, 64)
    {
        return None;
    }
    Some(session_id.to_owned())
}

fn parse_session_body(bytes: &[u8]) -> Result<String, FileHostError> {
    let text = std::str::from_utf8(bytes).map_err(|_| FileHostError::Corrupt)?;
    if !is_hex(text, 32) {
        return Err(FileHostError::Corrupt);
    }
    Ok(text.to_owned())
}

fn parse_canonical_u64(bytes: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(bytes).ok()?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if text.len() > 1 && text.starts_with('0') {
        return None;
    }
    if text.len() > 20 {
        return None;
    }
    text.parse().ok()
}

fn parse_store_key(key: &str) -> Result<Path, FileHostError> {
    Path::parse(key).map_err(|_| FileHostError::Store)
}

fn valid_identity(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_IDENTITY_BYTES {
        return false;
    }
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes.iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
    })
}

fn printable_text(value: &str) -> bool {
    !value.chars().any(|ch| ch.is_control())
}

fn valid_display_name(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_DISPLAY_BYTES || !printable_text(value) {
        return false;
    }
    !value.chars().any(|ch| ch == '/' || ch == '\\')
}

fn valid_content_type(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 3 || bytes.len() > MAX_CONTENT_TYPE_BYTES {
        return false;
    }
    let mut slash_at = None;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'/' {
            if slash_at.is_some() {
                return false;
            }
            slash_at = Some(index);
            continue;
        }
        if !(byte.is_ascii_alphanumeric() || matches!(*byte, b'+' | b'-' | b'.' | b'_')) {
            return false;
        }
    }
    let Some(slash) = slash_at else {
        return false;
    };
    slash != 0 && slash + 1 != bytes.len() && !value.contains("..")
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode_32(value: &str) -> Option<[u8; 32]> {
    if !is_hex(value, 64) {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        let hi = hex_nibble(value.as_bytes()[index * 2])?;
        let lo = hex_nibble(value.as_bytes()[index * 2 + 1])?;
        *byte = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn encode_session(record: &SessionRecord) -> Vec<u8> {
    let mut out = String::new();
    out.push('{');
    push_string(&mut out, "content_type", false);
    out.push(':');
    push_optional_string(&mut out, record.content_type.as_deref());
    push_string(&mut out, "display_name", true);
    out.push(':');
    push_json_string(&mut out, &record.display_name);
    push_string(&mut out, "expected_sha256", true);
    out.push(':');
    push_json_string(&mut out, &hex_encode(&record.expected_sha256));
    push_string(&mut out, "expected_size", true);
    out.push(':');
    out.push_str(&record.expected_size.to_string());
    push_string(&mut out, "grant_evidence", true);
    out.push(':');
    push_json_string(&mut out, &record.grant_evidence);
    push_string(&mut out, "idempotency_key", true);
    out.push(':');
    push_json_string(&mut out, &record.idempotency_key);
    push_string(&mut out, "object_key", true);
    out.push(':');
    push_json_string(&mut out, &record.object_key);
    push_string(&mut out, "principal_id", true);
    out.push(':');
    push_json_string(&mut out, &record.principal_id);
    push_string(&mut out, "project_id", true);
    out.push(':');
    push_json_string(&mut out, &record.project_id);
    push_string(&mut out, "session_id", true);
    out.push(':');
    push_json_string(&mut out, &record.session_id);
    push_string(&mut out, "version", true);
    out.push_str(":1}");
    out.into_bytes()
}

fn encode_manifest(manifest: &ObjectManifest) -> Vec<u8> {
    let mut out = String::new();
    out.push('{');
    push_string(&mut out, "content_type", false);
    out.push(':');
    push_optional_string(&mut out, manifest.content_type.as_deref());
    push_string(&mut out, "contract", true);
    out.push(':');
    push_json_string(&mut out, manifest.contract);
    push_string(&mut out, "display_name", true);
    out.push(':');
    push_json_string(&mut out, &manifest.display_name);
    push_string(&mut out, "grant_evidence", true);
    out.push(':');
    push_json_string(&mut out, manifest.grant_evidence.as_str());
    push_string(&mut out, "idempotency_key", true);
    out.push(':');
    push_json_string(&mut out, manifest.idempotency_key.as_str());
    push_string(&mut out, "lineage_digest", true);
    out.push(':');
    push_json_string(&mut out, &hex_encode(&manifest.lineage_digest));
    push_string(&mut out, "object_digest", true);
    out.push(':');
    push_json_string(&mut out, &hex_encode(&manifest.object_digest));
    push_string(&mut out, "object_key", true);
    out.push(':');
    push_json_string(&mut out, manifest.object_key.as_str());
    push_string(&mut out, "part_count", true);
    out.push(':');
    out.push_str(&manifest.part_count.to_string());
    push_string(&mut out, "principal_id", true);
    out.push(':');
    push_json_string(&mut out, manifest.principal_id.as_str());
    push_string(&mut out, "project_id", true);
    out.push(':');
    push_json_string(&mut out, manifest.project_id.as_str());
    push_string(&mut out, "session_id", true);
    out.push(':');
    push_json_string(&mut out, manifest.session_id.as_str());
    push_string(&mut out, "size", true);
    out.push(':');
    out.push_str(&manifest.size.to_string());
    push_string(&mut out, "version", true);
    out.push_str(":1}");
    out.into_bytes()
}

fn push_string(out: &mut String, key: &str, comma: bool) {
    if comma {
        out.push(',');
    }
    push_json_string(out, key);
}

fn push_optional_string(out: &mut String, value: Option<&str>) {
    match value {
        Some(value) => push_json_string(out, value),
        None => out.push_str("null"),
    }
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(ch),
        }
    }
    out.push('"');
}

fn decode_session(bytes: &[u8]) -> Result<SessionRecord, FileHostError> {
    let text = std::str::from_utf8(bytes).map_err(|_| FileHostError::Corrupt)?;
    let fields = parse_object(text)?;
    let record = SessionRecord {
        content_type: optional_string(&fields, "content_type")?,
        display_name: required_string(&fields, "display_name")?,
        expected_sha256: hex_decode_32(&required_string(&fields, "expected_sha256")?)
            .ok_or(FileHostError::Corrupt)?,
        expected_size: required_u64(&fields, "expected_size")?,
        grant_evidence: required_string(&fields, "grant_evidence")?,
        idempotency_key: required_string(&fields, "idempotency_key")?,
        object_key: required_string(&fields, "object_key")?,
        principal_id: required_string(&fields, "principal_id")?,
        project_id: required_string(&fields, "project_id")?,
        session_id: required_string(&fields, "session_id")?,
    };
    if required_u64(&fields, "version")? != 1
        || !is_hex(&record.session_id, 32)
        || !is_hex(&record.idempotency_key, 64)
        || record.object_key != object_key_for(&record.session_id, &record.expected_sha256)
        || !valid_identity(&record.project_id)
        || !valid_identity(&record.principal_id)
        || !valid_display_name(&record.display_name)
        || record.grant_evidence.is_empty()
        || record.grant_evidence.len() > MAX_GRANT_BYTES
        || !printable_text(&record.grant_evidence)
        || record
            .content_type
            .as_ref()
            .is_some_and(|value| !valid_content_type(value))
        || record.expected_size == 0
        || record.expected_size > MAX_OBJECT_BYTES
    {
        return Err(FileHostError::Corrupt);
    }
    if encode_session(&record) != bytes {
        return Err(FileHostError::Corrupt);
    }
    Ok(record)
}

fn decode_manifest(bytes: &[u8]) -> Result<ObjectManifest, FileHostError> {
    let text = std::str::from_utf8(bytes).map_err(|_| FileHostError::Corrupt)?;
    let fields = parse_object(text)?;
    if required_string(&fields, "contract")? != CONTRACT || required_u64(&fields, "version")? != 1 {
        return Err(FileHostError::Corrupt);
    }
    let record = SessionRecord {
        session_id: required_string(&fields, "session_id")?,
        project_id: required_string(&fields, "project_id")?,
        principal_id: required_string(&fields, "principal_id")?,
        grant_evidence: required_string(&fields, "grant_evidence")?,
        display_name: required_string(&fields, "display_name")?,
        content_type: optional_string(&fields, "content_type")?,
        expected_sha256: hex_decode_32(&required_string(&fields, "object_digest")?)
            .ok_or(FileHostError::Corrupt)?,
        expected_size: required_u64(&fields, "size")?,
        idempotency_key: required_string(&fields, "idempotency_key")?,
        object_key: required_string(&fields, "object_key")?,
    };
    let part_count = required_u64(&fields, "part_count")?;
    let part_count = u32::try_from(part_count).map_err(|_| FileHostError::Corrupt)?;
    let lineage = hex_decode_32(&required_string(&fields, "lineage_digest")?)
        .ok_or(FileHostError::Corrupt)?;
    if lineage
        != lineage_digest(
            &record,
            record.expected_sha256,
            record.expected_size,
            part_count,
        )
    {
        return Err(FileHostError::Corrupt);
    }
    let manifest = manifest_from_record(
        &record,
        record.expected_sha256,
        record.expected_size,
        part_count,
    );
    if encode_manifest(&manifest) != bytes {
        return Err(FileHostError::Corrupt);
    }
    Ok(manifest)
}

#[derive(Clone, Debug)]
enum JsonValue {
    Null,
    String(String),
    Number(u64),
}

fn required_string(
    fields: &BTreeMap<String, JsonValue>,
    key: &str,
) -> Result<String, FileHostError> {
    match fields.get(key) {
        Some(JsonValue::String(value)) => Ok(value.clone()),
        _ => Err(FileHostError::Corrupt),
    }
}

fn optional_string(
    fields: &BTreeMap<String, JsonValue>,
    key: &str,
) -> Result<Option<String>, FileHostError> {
    match fields.get(key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(value)) => Ok(Some(value.clone())),
        _ => Err(FileHostError::Corrupt),
    }
}

fn required_u64(fields: &BTreeMap<String, JsonValue>, key: &str) -> Result<u64, FileHostError> {
    match fields.get(key) {
        Some(JsonValue::Number(value)) => Ok(*value),
        _ => Err(FileHostError::Corrupt),
    }
}

fn parse_object(text: &str) -> Result<BTreeMap<String, JsonValue>, FileHostError> {
    let mut parser = Parser { text, index: 0 };
    parser.expect('{')?;
    let mut fields = BTreeMap::new();
    if parser.peek() == Some('}') {
        parser.bump();
        parser.finish()?;
        return Ok(fields);
    }
    loop {
        let key = parser.string()?;
        parser.expect(':')?;
        let value = parser.value()?;
        if fields.insert(key, value).is_some() {
            return Err(FileHostError::Corrupt);
        }
        match parser.bump() {
            Some(',') => continue,
            Some('}') => break,
            _ => return Err(FileHostError::Corrupt),
        }
    }
    parser.finish()?;
    Ok(fields)
}

struct Parser<'a> {
    text: &'a str,
    index: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.text[self.index..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += ch.len_utf8();
        Some(ch)
    }

    fn expect(&mut self, want: char) -> Result<(), FileHostError> {
        if self.bump() == Some(want) {
            Ok(())
        } else {
            Err(FileHostError::Corrupt)
        }
    }

    fn finish(&self) -> Result<(), FileHostError> {
        if self.index == self.text.len() {
            Ok(())
        } else {
            Err(FileHostError::Corrupt)
        }
    }

    fn value(&mut self) -> Result<JsonValue, FileHostError> {
        match self.peek() {
            Some('"') => Ok(JsonValue::String(self.string()?)),
            Some('n') => {
                self.literal("null")?;
                Ok(JsonValue::Null)
            }
            Some('0'..='9') => Ok(JsonValue::Number(self.number()?)),
            _ => Err(FileHostError::Corrupt),
        }
    }

    fn literal(&mut self, value: &str) -> Result<(), FileHostError> {
        if self.text[self.index..].starts_with(value) {
            self.index += value.len();
            Ok(())
        } else {
            Err(FileHostError::Corrupt)
        }
    }

    fn number(&mut self) -> Result<u64, FileHostError> {
        let start = self.index;
        while matches!(self.peek(), Some('0'..='9')) {
            self.bump();
        }
        let text = &self.text[start..self.index];
        parse_canonical_u64(text.as_bytes()).ok_or(FileHostError::Corrupt)
    }

    fn string(&mut self) -> Result<String, FileHostError> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.bump() {
                Some('"') => return Ok(out),
                Some('\\') => {
                    let escaped = self.bump().ok_or(FileHostError::Corrupt)?;
                    match escaped {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'u' => {
                            let rest = &self.text[self.index..];
                            if rest.len() < 4 {
                                return Err(FileHostError::Corrupt);
                            }
                            let hex = &rest[..4];
                            if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                                return Err(FileHostError::Corrupt);
                            }
                            let code =
                                u32::from_str_radix(hex, 16).map_err(|_| FileHostError::Corrupt)?;
                            let ch = char::from_u32(code).ok_or(FileHostError::Corrupt)?;
                            if ch.is_control() {
                                return Err(FileHostError::Corrupt);
                            }
                            out.push(ch);
                            self.index += 4;
                        }
                        _ => return Err(FileHostError::Corrupt),
                    }
                }
                Some(ch) if !ch.is_control() => out.push(ch),
                _ => return Err(FileHostError::Corrupt),
            }
        }
    }
}

#[cfg(test)]
mod delete_without_lock {
    use super::*;

    fn admission() -> AdmissionContext {
        AdmissionContext {
            project_id: ProjectId::new("project_a").unwrap(),
            principal_id: PrincipalId::new("user_a").unwrap(),
            grant_evidence: GrantEvidence::new("grant-a323").unwrap(),
        }
    }

    fn object_key() -> ObjectKey {
        ObjectKey::parse(
            "uploads/0123456789abcdef0123456789abcdef/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef/original",
        )
        .unwrap()
    }

    #[tokio::test]
    async fn new_returns_unsupported_before_store_io() {
        let dir = std::env::temp_dir().join(format!("oo-a323-nolock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = object_store::local::LocalFileSystem::new_with_prefix(&dir).unwrap();
        let host = FileHost::new(Arc::new(store), HostLimits::standard());
        let ctx = admission();
        let key = object_key();
        let digest = [1u8; 32];
        let lineage = [2u8; 32];
        assert_eq!(
            host.delete_completed(&ctx, &key, digest, 1, lineage, false)
                .await
                .unwrap_err(),
            FileHostError::Unsupported
        );
        assert_eq!(
            host.observe_completed_absence(&ctx, &key, digest, 1, lineage)
                .await
                .unwrap_err(),
            FileHostError::Unsupported
        );
        assert_eq!(
            host.completed_quota_release_count(&ctx, &key)
                .await
                .unwrap_err(),
            FileHostError::Unsupported
        );
        let session = SessionId::parse("0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(
            host.observe_open_absence(&ctx, &session).await.unwrap_err(),
            FileHostError::Unsupported
        );
        assert_eq!(
            host.abort(&ctx, &session).await.unwrap_err(),
            FileHostError::Unsupported
        );
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod retention_completion {
    use super::*;
    use object_store::ObjectStoreExt;

    fn admission(grant: &str) -> AdmissionContext {
        AdmissionContext {
            project_id: ProjectId::new("project_a").unwrap(),
            principal_id: PrincipalId::new("user_a").unwrap(),
            grant_evidence: GrantEvidence::new(grant).unwrap(),
        }
    }

    fn scratch(label: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let parent =
            std::env::temp_dir().join(format!("oo-a338-{label}-{}-{nanos}", std::process::id()));
        let root = parent.join("root");
        std::fs::create_dir_all(&root).unwrap();
        (parent, root)
    }

    fn config(name: &str, bytes: &[u8]) -> UploadConfig {
        UploadConfig {
            display_name: name.to_owned(),
            expected_sha256: sha256(bytes),
            expected_size: u64::try_from(bytes.len()).unwrap(),
            content_type: None,
        }
    }

    async fn publish(
        host: &FileHost,
        ctx: &AdmissionContext,
        bytes: &[u8],
        name: &str,
    ) -> ObjectManifest {
        let upload = config(name, bytes);
        let session = host.begin(ctx, &upload).await.unwrap();
        host.put_part(ctx, &session.session_id, 1, bytes)
            .await
            .unwrap();
        host.complete(ctx, &session.session_id).await.unwrap()
    }

    async fn leave_platform_staging(root: &std::path::Path, key: &str) {
        let store = object_store::local::LocalFileSystem::new_with_prefix(root).unwrap();
        let location = object_store::path::Path::parse(key).unwrap();
        let upload = store.put_multipart(&location).await.unwrap();
        std::mem::forget(upload);
    }

    fn quota_path(root: &std::path::Path, project: &str, session: &str) -> std::path::PathBuf {
        root.join(format!("meta/quota/{project}/{session}"))
    }

    fn legacy_body(manifest: &ObjectManifest, phase: &str, count: u8) -> String {
        let content_type = manifest.content_type.as_deref().unwrap_or("-");
        format!(
            "oo.control.local-file-hosting/v1 tombstone\nphase {phase}\nobject_key {}\nobject_digest {}\nsize {}\nlineage_digest {}\nproject {}\nprincipal {}\ngrant_hex {}\ndisplay_name {}\ncontent_type {content_type}\npart_count {}\nidempotency_key {}\nquota_key {}\nquota_bytes {}\nrelease_count {count}\n",
            manifest.object_key.as_str(),
            hex_encode(&manifest.object_digest),
            manifest.size,
            hex_encode(&manifest.lineage_digest),
            manifest.project_id.as_str(),
            manifest.principal_id.as_str(),
            hex_encode(manifest.grant_evidence.as_str().as_bytes()),
            manifest.display_name,
            manifest.part_count,
            manifest.idempotency_key.as_str(),
            quota_object_key_str(manifest.project_id.as_str(), manifest.session_id.as_str()),
            manifest.size
        )
    }

    #[tokio::test]
    async fn abort_reaps_platform_multipart_staging() {
        let (parent, root) = scratch("stage");
        let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
        let ctx = admission("grant-a338");
        let bytes = b"open-platform-part";
        let upload = config("open.txt", bytes);
        let session = host.begin(&ctx, &upload).await.unwrap();
        host.put_part(&ctx, &session.session_id, 1, bytes)
            .await
            .unwrap();
        leave_platform_staging(&root, session.object_key.as_str()).await;
        let part_key = format!("staging/{}/part/00001", session.session_id.as_str());
        leave_platform_staging(&root, &part_key).await;
        let part_dir = root.join(format!("staging/{}/part", session.session_id.as_str()));
        std::fs::write(part_dir.join("notes"), b"keep-notes").unwrap();
        std::fs::write(part_dir.join("dest#1"), b"keep-dest").unwrap();
        let odd = root.join(format!("{}#01", session.object_key.as_str()));
        std::fs::write(&odd, b"leading-zero").unwrap();
        let neighbor = parent.join("neighbor.txt");
        std::fs::write(&neighbor, b"neighbor").unwrap();
        assert!(
            root.join(format!("{}#1", session.object_key.as_str()))
                .is_file()
        );
        assert!(root.join(format!("{part_key}#1")).is_file());
        host.abort(&ctx, &session.session_id).await.unwrap();
        assert!(
            host.observe_open_absence(&ctx, &session.session_id)
                .await
                .unwrap()
        );
        assert!(
            !root
                .join(format!("{}#1", session.object_key.as_str()))
                .exists()
        );
        assert!(!root.join(format!("{part_key}#1")).exists());
        assert!(!root.join(&part_key).exists());
        assert_eq!(
            std::fs::read(part_dir.join("notes")).unwrap(),
            b"keep-notes"
        );
        assert_eq!(
            std::fs::read(part_dir.join("dest#1")).unwrap(),
            b"keep-dest"
        );
        assert_eq!(std::fs::read(&odd).unwrap(), b"leading-zero");
        assert_eq!(std::fs::read(&neighbor).unwrap(), b"neighbor");
        let wrong = admission("grant-other");
        assert_eq!(
            host.observe_open_absence(&wrong, &session.session_id)
                .await
                .unwrap_err(),
            FileHostError::Denied
        );
        assert_eq!(
            host.abort(&ctx, &session.session_id).await.unwrap_err(),
            FileHostError::NotFound
        );
        assert!(
            host.observe_open_absence(&ctx, &session.session_id)
                .await
                .unwrap()
        );
        drop(host);
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[tokio::test]
    async fn quota_stays_until_bytes_are_absent() {
        let (parent, root) = scratch("quota");
        let limits = HostLimits::try_new(64, 64, 10, 64).unwrap();
        let host = FileHost::open_local(&root, limits).unwrap();
        let ctx = admission("grant-a338");
        let bytes = vec![0x11; 64];
        let manifest = publish(&host, &ctx, &bytes, "held.txt").await;
        let quota = quota_path(
            &root,
            manifest.project_id.as_str(),
            manifest.session_id.as_str(),
        );
        assert_eq!(std::fs::read(&quota).unwrap(), b"64");
        let mut flipped = manifest.object_digest;
        flipped[0] ^= 0xff;
        assert_eq!(
            host.delete_completed(
                &ctx,
                &manifest.object_key,
                flipped,
                manifest.size,
                manifest.lineage_digest,
                false,
            )
            .await
            .unwrap_err(),
            FileHostError::NotFound
        );
        assert_eq!(std::fs::read(&quota).unwrap(), b"64");
        assert!(
            !root
                .join(format!("meta/tombstone/{}", manifest.session_id.as_str()))
                .exists()
        );
        let stopped = host
            .delete_completed(
                &ctx,
                &manifest.object_key,
                manifest.object_digest,
                manifest.size,
                manifest.lineage_digest,
                true,
            )
            .await
            .unwrap();
        assert!(stopped.is_none());
        assert_eq!(
            host.read_object(&ctx, &manifest.object_key)
                .await
                .unwrap_err(),
            FileHostError::NotFound
        );
        assert_eq!(std::fs::read(&quota).unwrap(), b"64");
        assert_eq!(
            host.completed_quota_release_count(&ctx, &manifest.object_key)
                .await
                .unwrap(),
            0
        );
        let tomb = std::fs::read_to_string(
            root.join(format!("meta/tombstone/{}", manifest.session_id.as_str())),
        )
        .unwrap();
        assert!(tomb.contains("phase data_absent\n"));
        assert!(tomb.contains("order bytes_then_quota\n"));
        assert!(tomb.contains("release_count 0\n"));
        assert!(
            !host
                .observe_completed_absence(
                    &ctx,
                    &manifest.object_key,
                    manifest.object_digest,
                    manifest.size,
                    manifest.lineage_digest,
                )
                .await
                .unwrap()
        );
        let other = config("other.txt", &bytes);
        assert_eq!(
            host.begin(&ctx, &other).await.unwrap_err(),
            FileHostError::Quota
        );
        let done = host
            .delete_completed(
                &ctx,
                &manifest.object_key,
                manifest.object_digest,
                manifest.size,
                manifest.lineage_digest,
                false,
            )
            .await
            .unwrap();
        assert!(done.is_some());
        assert!(!quota.exists());
        assert!(
            host.observe_completed_absence(
                &ctx,
                &manifest.object_key,
                manifest.object_digest,
                manifest.size,
                manifest.lineage_digest,
            )
            .await
            .unwrap()
        );
        assert_eq!(
            host.completed_quota_release_count(&ctx, &manifest.object_key)
                .await
                .unwrap(),
            1
        );
        let reused = host.begin(&ctx, &other).await.unwrap();
        assert_ne!(reused.session_id.as_str(), manifest.session_id.as_str());
        let again = host
            .delete_completed(
                &ctx,
                &manifest.object_key,
                manifest.object_digest,
                manifest.size,
                manifest.lineage_digest,
                false,
            )
            .await
            .unwrap();
        assert!(again.is_some());
        assert_eq!(
            host.completed_quota_release_count(&ctx, &manifest.object_key)
                .await
                .unwrap(),
            1
        );
        drop(host);
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[tokio::test]
    async fn legacy_quota_released_reconciles_without_a_second_release() {
        let (parent, root) = scratch("legacy");
        let limits = HostLimits::try_new(64, 64, 10, 64).unwrap();
        let host = FileHost::open_local(&root, limits).unwrap();
        let ctx = admission("grant-a338");
        let bytes = vec![0x22; 64];
        let manifest = publish(&host, &ctx, &bytes, "legacy.txt").await;
        let quota = quota_path(
            &root,
            manifest.project_id.as_str(),
            manifest.session_id.as_str(),
        );
        std::fs::remove_file(&quota).unwrap();
        let foreign_dir = root.join("meta/quota/project_b");
        std::fs::create_dir_all(&foreign_dir).unwrap();
        let foreign = foreign_dir.join("0123456789abcdef0123456789abcdef");
        std::fs::write(&foreign, b"64").unwrap();
        let tomb_path = root.join(format!("meta/tombstone/{}", manifest.session_id.as_str()));
        std::fs::create_dir_all(tomb_path.parent().unwrap()).unwrap();
        std::fs::write(&tomb_path, legacy_body(&manifest, "quota_released", 1)).unwrap();
        assert!(
            !host
                .observe_completed_absence(
                    &ctx,
                    &manifest.object_key,
                    manifest.object_digest,
                    manifest.size,
                    manifest.lineage_digest,
                )
                .await
                .unwrap()
        );
        let probe = config("probe.txt", &bytes);
        let opened = host.begin(&ctx, &probe).await.unwrap();
        assert_ne!(opened.session_id.as_str(), manifest.session_id.as_str());
        let probe_quota = quota_path(&root, "project_a", opened.session_id.as_str());
        assert_eq!(std::fs::read(&probe_quota).unwrap(), b"64");
        let done = host
            .delete_completed(
                &ctx,
                &manifest.object_key,
                manifest.object_digest,
                manifest.size,
                manifest.lineage_digest,
                false,
            )
            .await
            .unwrap();
        assert!(done.is_some());
        assert!(!root.join(manifest.object_key.as_str()).exists());
        assert!(!quota.exists());
        assert_eq!(std::fs::read(&foreign).unwrap(), b"64");
        assert_eq!(std::fs::read(&probe_quota).unwrap(), b"64");
        assert_eq!(
            host.completed_quota_release_count(&ctx, &manifest.object_key)
                .await
                .unwrap(),
            1
        );
        assert!(
            host.observe_completed_absence(
                &ctx,
                &manifest.object_key,
                manifest.object_digest,
                manifest.size,
                manifest.lineage_digest,
            )
            .await
            .unwrap()
        );
        let text = std::fs::read_to_string(&tomb_path).unwrap();
        assert!(text.contains("phase verified\n"));
        assert!(text.contains("order legacy_reconcile\n"));
        assert!(text.contains("release_count 1\n"));
        std::fs::create_dir_all(quota.parent().unwrap()).unwrap();
        std::fs::write(&quota, b"7").unwrap();
        assert_eq!(
            host.delete_completed(
                &ctx,
                &manifest.object_key,
                manifest.object_digest,
                manifest.size,
                manifest.lineage_digest,
                false,
            )
            .await
            .unwrap_err(),
            FileHostError::Conflict
        );
        assert_eq!(std::fs::read(&quota).unwrap(), b"7");
        assert_eq!(std::fs::read(&foreign).unwrap(), b"64");
        drop(host);
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[tokio::test]
    async fn symlink_staging_keeps_the_quota_and_writes_no_tombstone() {
        let (parent, root) = scratch("link");
        let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
        let ctx = admission("grant-a338");
        let bytes = b"symlink-target";
        let manifest = publish(&host, &ctx, bytes, "link.txt").await;
        let object = root.join(manifest.object_key.as_str());
        let staging = root.join(format!("{}#1", manifest.object_key.as_str()));
        std::os::unix::fs::symlink(&object, &staging).unwrap();
        assert_eq!(
            host.delete_completed(
                &ctx,
                &manifest.object_key,
                manifest.object_digest,
                manifest.size,
                manifest.lineage_digest,
                false,
            )
            .await
            .unwrap_err(),
            FileHostError::Corrupt
        );
        assert!(staging.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&object).unwrap(), bytes);
        let quota = quota_path(
            &root,
            manifest.project_id.as_str(),
            manifest.session_id.as_str(),
        );
        assert_eq!(
            std::fs::read(&quota).unwrap(),
            manifest.size.to_string().as_bytes()
        );
        assert!(
            !root
                .join(format!("meta/tombstone/{}", manifest.session_id.as_str()))
                .exists()
        );
        assert_eq!(
            host.completed_quota_release_count(&ctx, &manifest.object_key)
                .await
                .unwrap(),
            0
        );
        drop(host);
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[tokio::test]
    async fn mismatched_quota_body_writes_nothing() {
        let (parent, root) = scratch("body");
        let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
        let ctx = admission("grant-a338");
        let bytes = b"quota-body";
        let manifest = publish(&host, &ctx, bytes, "body.txt").await;
        let quota = quota_path(
            &root,
            manifest.project_id.as_str(),
            manifest.session_id.as_str(),
        );
        std::fs::write(&quota, b"99").unwrap();
        assert_eq!(
            host.delete_completed(
                &ctx,
                &manifest.object_key,
                manifest.object_digest,
                manifest.size,
                manifest.lineage_digest,
                false,
            )
            .await
            .unwrap_err(),
            FileHostError::Conflict
        );
        assert_eq!(std::fs::read(&quota).unwrap(), b"99");
        assert_eq!(
            host.read_object(&ctx, &manifest.object_key)
                .await
                .unwrap()
                .bytes,
            bytes
        );
        assert!(
            !root
                .join(format!("meta/tombstone/{}", manifest.session_id.as_str()))
                .exists()
        );
        drop(host);
        std::fs::remove_dir_all(&parent).unwrap();
    }
}
