//! Local multipart upload sessions on an [`object_store::ObjectStore`].
//!
//! The caller passes an [`AdmissionContext`] that a future control admission step
//! has already accepted. This module does not authenticate that context, does not
//! speak HTTP, and does not treat an unguessable key as authorization.
//!
//! Object keys and session ids are generated here. Callers never supply a
//! filesystem path for an upload, part, or read. [`FileHost::open_download`]
//! pulls a published object in bounded ranges. [`FileHost::open_local`] is the
//! operator entry that binds a deployment directory to [`object_store::local::LocalFileSystem`].
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
    GetOptions, GetRange, MultipartUpload, ObjectStore, ObjectStoreExt, PutMode,
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
        }
    }

    /// Open a local store rooted at `root`, creating the directory if needed.
    ///
    /// `root` is the deployment prefix, not an object key. Upload methods do not
    /// take filesystem paths. Writes fsync before success, matching the local
    /// store's durability switch for remote backends.
    pub fn open_local(root: &std::path::Path, limits: HostLimits) -> Result<Self, FileHostError> {
        std::fs::create_dir_all(root).map_err(|_| FileHostError::Store)?;
        let lock_path = deployment_lock_path(root)?;
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|_| FileHostError::Unavailable)?;
        let local = object_store::local::LocalFileSystem::new_with_prefix(root)
            .map_err(|_| FileHostError::Store)?
            .with_fsync(true);
        Ok(Self {
            store: Arc::new(local),
            limits,
            deployment_lock: Some(lock_path),
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
    /// set, is a caller clock checked before each ranged read and after each
    /// await. It is not a TTL, a revocation check, or a credential. The
    /// admission context is trusted input already accepted by a future control
    /// step. This method compares project and principal with the published
    /// manifest. It does not speak HTTP and it does not take a filesystem path.
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
        Ok(ObjectDownload {
            store: Arc::clone(&self.store),
            location: parse_store_key(object_key.as_str())?,
            manifest_key,
            manifest,
            manifest_bytes,
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
        let session_id = SessionId(record.session_id.clone());
        let parts = self.part_map(&session_id).await?;
        self.delete_parts(&session_id, &parts).await?;
        self.delete_key(&quota_object_key_str(
            &record.project_id,
            &record.session_id,
        ))
        .await?;
        self.delete_key(&idempotency_pointer_key_str(
            &record.project_id,
            &record.principal_id,
            &record.idempotency_key,
        )?)
        .await?;
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
}

enum PutOutcome {
    Created,
    Exists,
}

/// One bounded pull of a published object.
///
/// The manifest snapshot is fixed at open. [`ObjectDownload::is_complete`] becomes
/// true only after the pulled ranges match that snapshot's size and sha256 and the
/// stored manifest bytes are still the same. Stopping early, cancelling, passing
/// the deadline, or dropping the reader does not complete the pull. There is no
/// download credential and no [`std::io::Read`] cancel.
pub struct ObjectDownload {
    store: Arc<dyn ObjectStore>,
    location: Path,
    manifest_key: String,
    manifest: ObjectManifest,
    manifest_bytes: Vec<u8>,
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
        self.ensure_running()?;
        let pending = read_bounded_range(
            self.store.as_ref(),
            &self.location,
            self.offset,
            self.manifest.size,
            self.chunk_budget,
        )
        .await;
        self.ensure_running()?;
        let chunk = match pending {
            Ok(chunk) => chunk,
            Err(error) => return Err(self.fail(error)),
        };
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
        let loaded =
            load_capped_bytes(self.store.as_ref(), &self.manifest_key, MAX_MANIFEST_BYTES).await;
        self.ensure_running()?;
        let bytes = match loaded {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Err(self.fail(FileHostError::Corrupt)),
            Err(error) => return Err(self.fail(error)),
        };
        if bytes != self.manifest_bytes {
            return Err(self.fail(FileHostError::Corrupt));
        }
        self.ensure_running()?;
        let sized = store_len(self.store.as_ref(), self.manifest.object_key.as_str()).await;
        self.ensure_running()?;
        let found = match sized {
            Ok(found) => found,
            Err(error) => return Err(self.fail(error)),
        };
        if found != Some(self.manifest.size) || self.offset != self.manifest.size {
            return Err(self.fail(FileHostError::Corrupt));
        }
        if finalize(self.hasher.clone()) != self.manifest.object_digest {
            return Err(self.fail(FileHostError::Corrupt));
        }
        self.complete = true;
        Ok(())
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
