//! `oo.doc-source/v1`: read-only document sources for the Super Ontology
//! document pipeline (spec 10 §4.3.1, decisions D8, D10, D11).
//!
//! This is a new seam next to `SourceEngine`, not a change to it: database
//! sources are "table -> chunk -> watermark", documents are "object ->
//! fingerprint -> content hash". Backends (`fs`, OpenDAL `s3`, ...) live in
//! sibling modules owned by WS-BE1; this file is the shared contract and is
//! changed only by the integration thread (50 §4).
//!
//! Sources never write. Errors carry only static text so that object keys,
//! endpoints, and bucket names cannot leak through `Display` (10 §9).

use std::collections::BTreeSet;
use std::fmt;
use std::future::Future;

use serde::{Deserialize, Serialize};

pub const DOC_SOURCE_PROTOCOL: &str = "oo.doc-source/v1";
/// Maximum UTF-8 bytes of a whole Drive-relative path.
pub const MAX_PATH_BYTES: usize = 1024;
/// Maximum UTF-8 bytes of one path segment.
pub const MAX_SEGMENT_BYTES: usize = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocBackend {
    Fs,
    S3,
    Oss,
    Cos,
    R2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocCapability {
    List,
    Stat,
    RangedRead,
    Versioning,
    StrongEtag,
    Crc64,
    ConditionalRead,
    Changes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

/// A Drive-relative path: '/' separated, no empty, `.` or `..` segments, no
/// leading or trailing '/', no control characters, at most 1024 bytes with
/// segments of at most 255 bytes. Unicode NFC normalization is the caller's
/// job (the backend path module normalizes before constructing); this type
/// only enforces the structural rules so a `DocPath` is always safe to join
/// under a Drive root.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct DocPath(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocPathError {
    Empty,
    TooLong,
    SegmentTooLong,
    BadSegment,
    ControlCharacter,
}

impl DocPath {
    pub fn new(path: &str) -> Result<Self, DocPathError> {
        if path.is_empty() {
            return Err(DocPathError::Empty);
        }
        if path.len() > MAX_PATH_BYTES {
            return Err(DocPathError::TooLong);
        }
        if path.chars().any(char::is_control) {
            return Err(DocPathError::ControlCharacter);
        }
        for segment in path.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
                return Err(DocPathError::BadSegment);
            }
            if segment.len() > MAX_SEGMENT_BYTES {
                return Err(DocPathError::SegmentTooLong);
            }
        }
        Ok(Self(path.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }

    /// Parent directory, `None` for a top-level entry.
    pub fn parent(&self) -> Option<DocPath> {
        self.0
            .rsplit_once('/')
            .map(|(parent, _)| DocPath(parent.to_owned()))
    }
}

impl<'de> Deserialize<'de> for DocPath {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(de)?;
        DocPath::new(&raw).map_err(|_| serde::de::Error::custom("invalid Drive path"))
    }
}

impl fmt::Display for DocPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Change fingerprint (D10): any difference triggers a re-hash; equal content
/// hashes after a re-hash only refresh the fingerprint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub size: u64,
    pub etag: Option<String>,
    pub last_modified_ns: Option<i128>,
    pub version_id: Option<String>,
    pub fs_ino: Option<u64>,
    pub fs_ctime_ns: Option<i128>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectMeta {
    /// Source-side raw bytes (S3 key or `OsStr` bytes relative to the root).
    pub raw_key: Vec<u8>,
    /// `None` when the key cannot be normalized: the file becomes
    /// `unsupported(DOCS_SOURCE_PATH_INVALID)`.
    pub path: Option<DocPath>,
    pub kind: EntryKind,
    pub size: u64,
    pub etag: Option<String>,
    pub last_modified_ns: Option<i128>,
    pub version_id: Option<String>,
    pub crc64: Option<u64>,
    pub fs_ino: Option<u64>,
    pub fs_ctime_ns: Option<i128>,
}

impl ObjectMeta {
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            size: self.size,
            etag: self.etag.clone(),
            last_modified_ns: self.last_modified_ns,
            version_id: self.version_id.clone(),
            fs_ino: self.fs_ino,
            fs_ctime_ns: self.fs_ctime_ns,
        }
    }
}

/// Opaque backend cursor for paginated listing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListCursor(pub String);

/// Opaque backend cursor for change hints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeCursor(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListPage {
    pub entries: Vec<ObjectMeta>,
    pub next: Option<ListCursor>,
    /// True only on the last page of a full listing. A listing that is not
    /// complete must never trigger a sweep (10 §4.3.4).
    pub complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOp {
    Put,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeHint {
    pub raw_key: Vec<u8>,
    pub op: ChangeOp,
    pub version_id: Option<String>,
    pub sequencer: Option<String>,
    pub observed_at_ms: i64,
}

/// Conditions for a read. When set and the object changed, the read returns
/// [`DocSourceError::Changed`] instead of mixed bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadExpect {
    pub etag: Option<String>,
    pub version_id: Option<String>,
    pub size: Option<u64>,
}

/// Closed error set. `Display` is static text only (no keys, endpoints, or
/// bucket names), which is the first layer of log redaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocSourceError {
    NotFound,
    PermissionDenied,
    AuthFailed,
    Throttled { retry_after_ms: u64 },
    Unavailable,
    Timeout,
    Changed,
    InvalidConfig(&'static str),
    Protocol(&'static str),
    Io(&'static str),
}

impl DocSourceError {
    /// Stable `DOCS_SOURCE_*` machine code (10 §8.4).
    pub fn code(&self) -> &'static str {
        match self {
            DocSourceError::NotFound => "DOCS_SOURCE_NOT_FOUND",
            DocSourceError::PermissionDenied => "DOCS_SOURCE_PERMISSION_DENIED",
            DocSourceError::AuthFailed => "DOCS_SOURCE_AUTH_FAILED",
            DocSourceError::Throttled { .. } => "DOCS_SOURCE_THROTTLED",
            DocSourceError::Unavailable => "DOCS_SOURCE_UNAVAILABLE",
            DocSourceError::Timeout => "DOCS_SOURCE_TIMEOUT",
            DocSourceError::Changed => "DOCS_SOURCE_CHANGED_DURING_READ",
            DocSourceError::InvalidConfig(_) => "DOCS_SOURCE_INVALID_CONFIG",
            DocSourceError::Protocol(_) => "DOCS_SOURCE_PROTOCOL",
            DocSourceError::Io(_) => "DOCS_SOURCE_IO",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self,
            DocSourceError::Throttled { .. }
                | DocSourceError::Unavailable
                | DocSourceError::Timeout
                | DocSourceError::Changed
                | DocSourceError::Io(_)
        )
    }
}

impl fmt::Display for DocSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocSourceError::NotFound => f.write_str("object not found"),
            DocSourceError::PermissionDenied => f.write_str("permission denied"),
            DocSourceError::AuthFailed => f.write_str("authentication failed"),
            DocSourceError::Throttled { .. } => f.write_str("throttled by the source"),
            DocSourceError::Unavailable => f.write_str("source unavailable"),
            DocSourceError::Timeout => f.write_str("source timed out"),
            DocSourceError::Changed => f.write_str("object changed during read"),
            DocSourceError::InvalidConfig(why)
            | DocSourceError::Protocol(why)
            | DocSourceError::Io(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for DocSourceError {}

/// A read-only document source. Implementations must not follow symbolic
/// links and must not write to the source.
pub trait DocumentSource: Send + Sync {
    fn backend(&self) -> DocBackend;
    fn capabilities(&self) -> BTreeSet<DocCapability>;
    /// Paginated listing; `complete = true` only on the last page.
    fn list(
        &self,
        cursor: Option<&ListCursor>,
        page_size: u32,
    ) -> impl Future<Output = Result<ListPage, DocSourceError>> + Send;
    fn stat(
        &self,
        raw_key: &[u8],
    ) -> impl Future<Output = Result<Option<ObjectMeta>, DocSourceError>> + Send;
    /// Read `[offset, offset + len)`. With `expect` set, a changed object
    /// returns [`DocSourceError::Changed`].
    fn read(
        &self,
        raw_key: &[u8],
        offset: u64,
        len: u64,
        expect: &ReadExpect,
    ) -> impl Future<Output = Result<Vec<u8>, DocSourceError>> + Send;
    /// Change hints only accelerate; listing reconciliation is authoritative
    /// (D10). Default: unsupported.
    fn changes(
        &self,
        _since: Option<&ChangeCursor>,
    ) -> impl Future<Output = Result<Option<(Vec<ChangeHint>, ChangeCursor)>, DocSourceError>> + Send
    {
        async { Ok(None) }
    }
}

#[cfg(test)]
mod contract_tests;
