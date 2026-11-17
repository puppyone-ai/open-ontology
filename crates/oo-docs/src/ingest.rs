//! Listing reconciliation, fingerprints, hashing (10 §4.3). Board WS-BE1.
//!
//! A `sync` job lists a Drive and compares it with the `file` table
//! ([`reconcile`]): new paths and changed fingerprints become `hash` jobs
//! ([`hashjob`]); a complete listing then sweeps the files it did not see
//! (generation mark-sweep, 4.3.4). A hash job streams the object through
//! BLAKE3 and SHA-256, writes the `blob` and `file_version` rows and moves the
//! file `stored -> queued` (or `duplicate`, `unsupported`, `encrypted`), in
//! one transaction with the job's own completion (4.1.3). [`handlers`] binds
//! both to the scheduler in `jobs::scheduler`; [`poll`] decides when to
//! enqueue syncs.
//!
//! Formal: FM3 (reconcile). The sweep runs only after a listing whose last
//! page said `complete`, never on a targeted sync, and never when the
//! missing set is a mass delete that nobody confirmed.

use std::collections::BTreeSet;
use std::fmt;
use std::future::Future;
use std::time::Duration;

use oo_connectors::objectstore::fs::FsSource;
use oo_connectors::objectstore::hash::DualDigest;
use oo_connectors::objectstore::path::ExcludeSet;
use oo_connectors::objectstore::{DocBackend, DocSourceError, DocumentSource, ObjectMeta};
use sha2::{Digest, Sha256};

use crate::state::TransitionError;
use crate::store::StoreError;

pub mod files;
pub mod handlers;
pub mod hashjob;
pub mod poll;
#[cfg(kani)]
mod proofs;
pub mod reconcile;
pub mod sniff;
pub mod zipguard;

pub use sniff::Format;

/// A [`DocumentSource`] that can also stream-hash an object. `hash_object`
/// stats before and after and returns [`DocSourceError::Changed`] when the
/// object moved underneath it.
pub trait HashableSource: DocumentSource {
    /// BLAKE3 + SHA-256 of the whole object plus the metadata it had.
    fn hash_object(
        &self,
        raw_key: &[u8],
    ) -> impl Future<Output = Result<(DualDigest, ObjectMeta), DocSourceError>> + Send;
}

impl HashableSource for FsSource {
    fn hash_object(
        &self,
        raw_key: &[u8],
    ) -> impl Future<Output = Result<(DualDigest, ObjectMeta), DocSourceError>> + Send {
        FsSource::hash_object(self, raw_key)
    }
}

/// What the engine knows about a Drive when it opens its source.
#[derive(Clone, Debug)]
pub struct DriveRef {
    /// Drive id (UUID text).
    pub id: String,
    /// Backend text (`fs`, `s3`, ...).
    pub backend: String,
    /// `drive.config` parsed.
    pub config: serde_json::Value,
    /// `drive.status`.
    pub status: String,
    /// Committed generation.
    pub sync_generation: i64,
    /// `drive.max_files`.
    pub max_files: i64,
}

/// Opens the [`HashableSource`] of a Drive. One value serves every Drive.
pub trait SourceFactory: Send + Sync + 'static {
    /// The concrete source.
    type Source: HashableSource + 'static;
    /// Open it; errors map to the sync failing with that `DOCS_SOURCE_*` code.
    fn open(&self, drive: &DriveRef) -> Result<Self::Source, DocSourceError>;
}

/// Factory for `fs` Drives: `config.root` plus `config.exclude` globs.
#[derive(Clone, Copy, Debug, Default)]
pub struct FsFactory;

impl SourceFactory for FsFactory {
    type Source = FsSource;

    fn open(&self, drive: &DriveRef) -> Result<FsSource, DocSourceError> {
        if drive.backend != DocBackend::Fs.as_db() {
            return Err(DocSourceError::InvalidConfig("backend is not fs"));
        }
        let root = drive
            .config
            .get("root")
            .and_then(|v| v.as_str())
            .ok_or(DocSourceError::InvalidConfig("fs config has no root"))?;
        let excludes = drive
            .config
            .get("exclude")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        FsSource::open(std::path::Path::new(root), ExcludeSet::new(excludes))
    }
}

trait BackendText {
    fn as_db(&self) -> &'static str;
}

impl BackendText for DocBackend {
    fn as_db(&self) -> &'static str {
        match self {
            DocBackend::Fs => "fs",
            DocBackend::S3 => "s3",
            DocBackend::Oss => "oss",
            DocBackend::Cos => "cos",
            DocBackend::R2 => "r2",
        }
    }
}

/// Knobs from `docs.json` (10 §2.8, §4.11) with the spec defaults.
#[derive(Clone, Debug)]
pub struct IngestLimits {
    /// Largest object that is hashed (`too_large` above it).
    pub max_file_bytes: u64,
    /// Same for md and txt.
    pub max_text_file_bytes: u64,
    /// Wait before a recently written file is hashed (`settle_seconds`).
    pub settle: Duration,
    /// Listing page size; also the per-page cap on queued hash jobs (4.1.7).
    pub page_size: u32,
    /// With more queued hash jobs than this a sync stops enqueueing (4.1.7).
    pub hash_backlog_pause: i64,
    /// A missing set must exceed this and half of the live files to be held.
    pub mass_delete_min: usize,
    /// New files hashed inline per sync to find renames.
    pub rename_candidate_cap: usize,
    /// Settle retries that do not count as an attempt (4.3.2).
    pub unstable_limit: i64,
    /// Formats a parser exists for; the others become `unsupported`.
    pub parsers: BTreeSet<Format>,
}

impl Default for IngestLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 200 * 1024 * 1024,
            max_text_file_bytes: 64 * 1024 * 1024,
            settle: Duration::from_secs(2),
            page_size: 1000,
            hash_backlog_pause: 5000,
            mass_delete_min: 100,
            rename_candidate_cap: 200,
            unstable_limit: 10,
            parsers: BTreeSet::from([Format::Md, Format::Txt]),
        }
    }
}

impl IngestLimits {
    /// Size ceiling for a path, by its extension.
    pub fn size_limit(&self, path: &str) -> u64 {
        match sniff::extension(path).as_str() {
            "md" | "markdown" | "mdx" | "txt" => self.max_text_file_bytes,
            _ => self.max_file_bytes,
        }
    }
}

/// Who asked for a sync (`drive_sync.trigger`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// Periodic poll.
    Poll,
    /// `POST :sync`.
    Manual,
    /// Source event hint.
    Event,
    /// Engine start.
    Startup,
    /// Weekly deep hash audit.
    Audit,
}

impl Trigger {
    /// Database text.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Poll => "poll",
            Self::Manual => "manual",
            Self::Event => "event",
            Self::Startup => "startup",
            Self::Audit => "audit",
        }
    }
}

/// Variant of a sync; encoded in the job's dedupe key so that the job row
/// is all the scheduler needs (jobs have no payload column).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncFlavor {
    /// Trigger.
    pub trigger: Trigger,
    /// Ignore fingerprints and rehash everything (`deep`, weekly audit).
    pub deep: bool,
    /// Release a held mass delete of this generation.
    pub confirm: Option<i64>,
}

impl SyncFlavor {
    /// Plain periodic sync.
    pub const POLL: SyncFlavor = SyncFlavor {
        trigger: Trigger::Poll,
        deep: false,
        confirm: None,
    };

    /// `sync:<drive>:<generation>[:<flavor>]`.
    pub fn dedupe_key(&self, drive_id: &str, generation: i64) -> String {
        let mut key = format!("sync:{drive_id}:{generation}");
        match (self.trigger, self.deep, self.confirm) {
            (_, _, Some(_)) => key.push_str(":confirm"),
            (Trigger::Audit, _, _) => key.push_str(":audit"),
            (_, true, _) => key.push_str(":deep"),
            (Trigger::Manual, ..) => key.push_str(":manual"),
            (Trigger::Event, ..) => key.push_str(":event"),
            (Trigger::Startup, ..) => key.push_str(":startup"),
            (Trigger::Poll, ..) => {}
        }
        key
    }

    /// Inverse of [`SyncFlavor::dedupe_key`]; `#retry` suffixes are ignored.
    pub fn from_key(key: &str) -> Option<(String, i64, SyncFlavor)> {
        let key = key.split('#').next()?;
        let mut parts = key.split(':');
        if parts.next()? != "sync" {
            return None;
        }
        let drive = parts.next()?.to_owned();
        let generation: i64 = parts.next()?.parse().ok()?;
        let flavor = match parts.next() {
            None => SyncFlavor::POLL,
            Some("manual") => SyncFlavor {
                trigger: Trigger::Manual,
                ..SyncFlavor::POLL
            },
            Some("event") => SyncFlavor {
                trigger: Trigger::Event,
                ..SyncFlavor::POLL
            },
            Some("startup") => SyncFlavor {
                trigger: Trigger::Startup,
                ..SyncFlavor::POLL
            },
            Some("deep") => SyncFlavor {
                trigger: Trigger::Manual,
                deep: true,
                confirm: None,
            },
            Some("audit") => SyncFlavor {
                trigger: Trigger::Audit,
                deep: true,
                confirm: None,
            },
            // The confirmed generation is the one in the key.
            Some("confirm") => SyncFlavor {
                trigger: Trigger::Manual,
                deep: false,
                confirm: Some(generation),
            },
            Some(_) => return None,
        };
        Some((drive, generation, flavor))
    }
}

/// Ingest failure that is not a normal sync outcome.
#[derive(Debug)]
pub enum IngestError {
    /// Database.
    Store(StoreError),
    /// State machine refused or failed.
    Transition(TransitionError),
    /// Another sync of this Drive holds the lock.
    SyncRunning,
    /// No such Drive.
    DriveNotFound,
    /// Drive is paused, deleting or deleted.
    DriveNotRunnable(String),
    /// `confirm_mass_delete` named another generation.
    ConfirmMismatch {
        /// Generation that is actually held.
        generation: i64,
    },
    /// A row did not have the promised shape.
    Protocol(String),
}

impl fmt::Display for IngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(e) => e.fmt(f),
            Self::Transition(e) => e.fmt(f),
            Self::SyncRunning => f.write_str("a sync of this drive is already running"),
            Self::DriveNotFound => f.write_str("drive not found"),
            Self::DriveNotRunnable(s) => write!(f, "drive is {s}"),
            Self::ConfirmMismatch { generation } => {
                write!(f, "held generation is {generation}")
            }
            Self::Protocol(m) => write!(f, "ingest protocol error: {m}"),
        }
    }
}

impl std::error::Error for IngestError {}

impl From<StoreError> for IngestError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

impl From<sqlx::Error> for IngestError {
    fn from(e: sqlx::Error) -> Self {
        Self::Store(e.into())
    }
}

impl From<TransitionError> for IngestError {
    fn from(e: TransitionError) -> Self {
        Self::Transition(e)
    }
}

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}

/// Inverse of [`hex`]; `None` on bad input.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let nib = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    text.as_bytes()
        .chunks(2)
        .map(|p| Some(nib(p[0])? << 4 | nib(p[1])?))
        .collect()
}

/// Hash of the fingerprint fields, the tail of `hash:<file_id>:<fp_sha256>`.
pub fn fp_sha256(fp: &oo_connectors::objectstore::Fingerprint) -> String {
    let text = format!(
        "{}|{:?}|{:?}|{:?}|{:?}|{:?}",
        fp.size, fp.last_modified_ns, fp.fs_ino, fp.fs_ctime_ns, fp.etag, fp.version_id
    );
    hex(&Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flavor_keys_round_trip() {
        let drive = "0f8fad5b-d9cb-469f-a165-70867728950e";
        let all = [
            SyncFlavor::POLL,
            SyncFlavor {
                trigger: Trigger::Manual,
                deep: false,
                confirm: None,
            },
            SyncFlavor {
                trigger: Trigger::Manual,
                deep: true,
                confirm: None,
            },
            SyncFlavor {
                trigger: Trigger::Audit,
                deep: true,
                confirm: None,
            },
            SyncFlavor {
                trigger: Trigger::Startup,
                deep: false,
                confirm: None,
            },
            SyncFlavor {
                trigger: Trigger::Manual,
                deep: false,
                confirm: Some(7),
            },
        ];
        for f in all {
            let key = f.dedupe_key(drive, 7);
            let (d, g, back) = SyncFlavor::from_key(&format!("{key}#retry2")).unwrap();
            assert_eq!((d.as_str(), g, back), (drive, 7, f), "{key}");
        }
        assert!(SyncFlavor::from_key("hash:x:1").is_none());
        assert!(SyncFlavor::from_key("sync:d:notnum").is_none());
    }

    #[test]
    fn hex_round_trips() {
        let raw = [0u8, 1, 0xab, 0xff];
        assert_eq!(hex(&raw), "0001abff");
        assert_eq!(unhex("0001abff").unwrap(), raw);
        assert!(unhex("abc").is_none());
        assert!(unhex("zz").is_none());
    }

    #[test]
    fn size_limit_by_extension() {
        let l = IngestLimits::default();
        assert_eq!(l.size_limit("a/b.MD"), 64 * 1024 * 1024);
        assert_eq!(l.size_limit("a.pdf"), 200 * 1024 * 1024);
    }
}
