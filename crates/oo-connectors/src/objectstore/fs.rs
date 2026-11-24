//! Read-only filesystem backend (`oo.doc-source/v1`, spec 10 §4.3.2).
//!
//! Safety model. Workspace code forbids `unsafe`, and `libc` is not a
//! dependency of this crate, so `openat(O_NOFOLLOW)` per component is not
//! available. In its place every access does the following.
//!
//! - Validates the raw key (no `..`, no absolute, no NUL).
//! - Applies the skip table (hidden, junk, `.context`), so those names are
//!   neither listed, stat-able nor readable.
//! - Runs `symlink_metadata` on EVERY component from the root down and fails
//!   closed (`PermissionDenied`) if any component is a symlink.
//! - Opens the file, then compares the open handle's device and inode with the
//!   pre-open `symlink_metadata` (a last-component swap to a symlink makes
//!   them differ) and requires a regular file.
//! - Re-canonicalizes after open and requires the result to stay under the
//!   canonical root.
//!
//! Known limit: an attacker with write access inside the Drive can still win a
//! race that swaps an INTERMEDIATE directory for a symlink between the
//! component check and the open (TOCTOU); the last two checks narrow but do
//! not close it. Closing it needs `openat`/`O_NOFOLLOW`, i.e. a `libc`
//! dependency (coordinator decision, see the B-S2-02 report). The Drive root is
//! operator-owned and the engine container mounts it read-only, which is the
//! compensating control.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::hash::{DualDigest, hash_reader};
use super::path::ignore::{IgnoreStack, load_dir_rules};
use super::path::{ExcludeSet, normalize_raw_key, raw_key_is_safe, skip_reason, skip_reason_fs};
use super::{
    DocBackend, DocCapability, DocSourceError, DocumentSource, EntryKind, ListCursor, ListPage,
    ObjectMeta, ReadExpect,
};

/// Largest single ranged read.
pub const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;
/// Deepest directory nesting walked; deeper subtrees mark the listing
/// incomplete (so nothing is swept).
pub const MAX_DEPTH: usize = 64;
const MAX_PAGE: u32 = 10_000;

#[derive(Debug)]
struct Inner {
    /// Canonical root.
    root: PathBuf,
    excludes: ExcludeSet,
    skipped_special: AtomicU64,
}

/// A filesystem Drive root exposed as a read-only [`DocumentSource`].
#[derive(Clone, Debug)]
pub struct FsSource {
    inner: Arc<Inner>,
}

impl FsSource {
    /// `root` must exist and be a directory (it may itself be a symlink or
    /// bind mount; it is resolved once, here, and never again).
    pub fn open(root: &Path, excludes: ExcludeSet) -> Result<Self, DocSourceError> {
        let canon = std::fs::canonicalize(root).map_err(|_| root_missing())?;
        if !std::fs::metadata(&canon).is_ok_and(|m| m.is_dir()) {
            return Err(root_missing());
        }
        Ok(Self {
            inner: Arc::new(Inner {
                root: canon,
                excludes,
                skipped_special: AtomicU64::new(0),
            }),
        })
    }

    /// FIFOs, sockets and devices seen (and skipped) by listings so far.
    pub fn skipped_special(&self) -> u64 {
        self.inner.skipped_special.load(Ordering::Relaxed)
    }

    /// Stream-hash an object with BLAKE3 + SHA-256. The object is stat-ed
    /// before and after; a change in between returns
    /// [`DocSourceError::Changed`] (the caller requeues without counting an
    /// attempt, 10 §4.3.2). The settle wait is the scheduler's job.
    pub async fn hash_object(
        &self,
        raw_key: &[u8],
    ) -> Result<(DualDigest, ObjectMeta), DocSourceError> {
        let inner = self.inner.clone();
        let key = raw_key.to_vec();
        run_blocking(move || inner.hash_object(&key)).await
    }
}

fn root_missing() -> DocSourceError {
    DocSourceError::InvalidConfig("source root is missing or not a directory")
}

async fn run_blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, DocSourceError> + Send + 'static,
) -> Result<T, DocSourceError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| DocSourceError::Io("blocking task failed"))?
}

fn map_io(e: &std::io::Error) -> DocSourceError {
    match e.kind() {
        std::io::ErrorKind::NotFound => DocSourceError::NotFound,
        std::io::ErrorKind::PermissionDenied => DocSourceError::PermissionDenied,
        _ => DocSourceError::Io("filesystem operation failed"),
    }
}

#[cfg(unix)]
fn raw_bytes(name: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    name.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn raw_bytes(name: &OsStr) -> Vec<u8> {
    name.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
fn os_from_bytes(bytes: &[u8]) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStrExt;
    OsStr::from_bytes(bytes).to_os_string()
}

#[cfg(not(unix))]
fn os_from_bytes(bytes: &[u8]) -> std::ffi::OsString {
    std::ffi::OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

fn kind_of(meta: &Metadata) -> EntryKind {
    let ft = meta.file_type();
    if ft.is_symlink() {
        EntryKind::Symlink
    } else if ft.is_dir() {
        EntryKind::Dir
    } else if ft.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

#[cfg(unix)]
fn unix_parts(meta: &Metadata) -> (Option<i128>, Option<u64>, Option<i128>) {
    use std::os::unix::fs::MetadataExt;
    let mtime = i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec());
    let ctime = i128::from(meta.ctime()) * 1_000_000_000 + i128::from(meta.ctime_nsec());
    (Some(mtime), Some(meta.ino()), Some(ctime))
}

#[cfg(not(unix))]
fn unix_parts(meta: &Metadata) -> (Option<i128>, Option<u64>, Option<i128>) {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i128);
    (mtime, None, None)
}

#[cfg(unix)]
fn same_file(a: &Metadata, b: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(not(unix))]
fn same_file(a: &Metadata, b: &Metadata) -> bool {
    a.len() == b.len()
}

fn meta_of(raw_key: Vec<u8>, md: &Metadata) -> ObjectMeta {
    let (mtime, ino, ctime) = unix_parts(md);
    ObjectMeta {
        path: normalize_raw_key(&raw_key),
        raw_key,
        kind: kind_of(md),
        size: md.len(),
        etag: None,
        last_modified_ns: mtime,
        version_id: None,
        crc64: None,
        fs_ino: ino,
        fs_ctime_ns: ctime,
    }
}

fn join_key(root: &Path, raw_key: &[u8]) -> PathBuf {
    let mut out = root.to_path_buf();
    for seg in raw_key.split(|b| *b == b'/') {
        out.push(os_from_bytes(seg));
    }
    out
}

impl Inner {
    fn key_excluded(&self, raw_key: &[u8]) -> bool {
        let segments_skipped = raw_key
            .split(|b| *b == b'/')
            .any(|s| skip_reason(&String::from_utf8_lossy(s)).is_some());
        if segments_skipped {
            return true;
        }
        match normalize_raw_key(raw_key) {
            Some(p) => self.excludes.excludes(p.as_str()),
            None => false,
        }
    }

    /// Validate the key and walk every component with `symlink_metadata`.
    /// Returns the absolute path and the metadata of the final component.
    fn checked_lstat(&self, raw_key: &[u8]) -> Result<(PathBuf, Metadata), DocSourceError> {
        if !raw_key_is_safe(raw_key) {
            return Err(DocSourceError::Protocol("invalid object key"));
        }
        if self.key_excluded(raw_key) {
            return Err(DocSourceError::NotFound);
        }
        let segments: Vec<&[u8]> = raw_key.split(|b| *b == b'/').collect();
        let mut cur = self.root.clone();
        let mut last: Option<Metadata> = None;
        for (i, seg) in segments.iter().enumerate() {
            cur.push(os_from_bytes(seg));
            let md = std::fs::symlink_metadata(&cur).map_err(|e| map_io(&e))?;
            if md.file_type().is_symlink() {
                // Fail closed: never follow, never resolve.
                return Err(DocSourceError::PermissionDenied);
            }
            if i + 1 < segments.len() && !md.is_dir() {
                return Err(DocSourceError::NotFound);
            }
            last = Some(md);
        }
        let md = last.ok_or(DocSourceError::Protocol("invalid object key"))?;
        Ok((cur, md))
    }

    fn open_regular(&self, raw_key: &[u8]) -> Result<(File, Metadata), DocSourceError> {
        let (abs, lmd) = self.checked_lstat(raw_key)?;
        if !lmd.is_file() {
            return Err(DocSourceError::NotFound);
        }
        let file = File::open(&abs).map_err(|e| map_io(&e))?;
        let fmd = file.metadata().map_err(|e| map_io(&e))?;
        if !fmd.is_file() || !same_file(&lmd, &fmd) {
            return Err(DocSourceError::Changed);
        }
        let canon = std::fs::canonicalize(&abs).map_err(|e| map_io(&e))?;
        if !canon.starts_with(&self.root) {
            return Err(DocSourceError::PermissionDenied);
        }
        Ok((file, fmd))
    }

    fn stat(&self, raw_key: &[u8]) -> Result<Option<ObjectMeta>, DocSourceError> {
        match self.checked_lstat(raw_key) {
            Ok((_, md)) => Ok(Some(meta_of(raw_key.to_vec(), &md))),
            Err(DocSourceError::NotFound) => Ok(None),
            Err(DocSourceError::PermissionDenied) => {
                // A symlink as the last component is a legitimate listing
                // entry; report it as such instead of failing.
                self.stat_final_symlink(raw_key)
            }
            Err(e) => Err(e),
        }
    }

    fn stat_final_symlink(&self, raw_key: &[u8]) -> Result<Option<ObjectMeta>, DocSourceError> {
        let (parent, name) = match raw_key.iter().rposition(|b| *b == b'/') {
            Some(i) => (&raw_key[..i], &raw_key[i + 1..]),
            None => (&raw_key[..0], raw_key),
        };
        if !parent.is_empty() {
            match self.checked_lstat(parent) {
                Ok((_, md)) if md.is_dir() => {}
                _ => return Err(DocSourceError::PermissionDenied),
            }
        }
        let abs = join_key(&self.root, parent).join(os_from_bytes(name));
        let md = std::fs::symlink_metadata(&abs).map_err(|e| map_io(&e))?;
        if md.file_type().is_symlink() {
            Ok(Some(meta_of(raw_key.to_vec(), &md)))
        } else {
            Err(DocSourceError::PermissionDenied)
        }
    }

    fn read(
        &self,
        raw_key: &[u8],
        offset: u64,
        len: u64,
        expect: &ReadExpect,
    ) -> Result<Vec<u8>, DocSourceError> {
        if len > MAX_READ_BYTES {
            return Err(DocSourceError::Protocol("read length exceeds the bound"));
        }
        let (mut file, before) = self.open_regular(raw_key)?;
        if expect.size.is_some_and(|s| s != before.len()) {
            return Err(DocSourceError::Changed);
        }
        let want = len.min(before.len().saturating_sub(offset));
        file.seek(SeekFrom::Start(offset)).map_err(|e| map_io(&e))?;
        let mut buf = Vec::with_capacity(want as usize);
        (&file)
            .take(want)
            .read_to_end(&mut buf)
            .map_err(|e| map_io(&e))?;
        let after = file.metadata().map_err(|e| map_io(&e))?;
        if !unchanged(&before, &after) {
            return Err(DocSourceError::Changed);
        }
        Ok(buf)
    }

    fn hash_object(&self, raw_key: &[u8]) -> Result<(DualDigest, ObjectMeta), DocSourceError> {
        let (file, before) = self.open_regular(raw_key)?;
        let digest = hash_reader(&file).map_err(|e| map_io(&e))?;
        let after = file.metadata().map_err(|e| map_io(&e))?;
        if !unchanged(&before, &after) || digest.len != after.len() {
            return Err(DocSourceError::Changed);
        }
        Ok((digest, meta_of(raw_key.to_vec(), &after)))
    }
}

/// Size, mtime and ctime unchanged between two stats of the same handle.
pub fn unchanged(before: &Metadata, after: &Metadata) -> bool {
    let (m0, _, c0) = unix_parts(before);
    let (m1, _, c1) = unix_parts(after);
    before.len() == after.len() && m0 == m1 && c0 == c1
}

/// Settled when size and mtime are identical across the settle wait (10
/// §4.3.2). Pure so the scheduler and the tests share one definition.
pub fn is_settled(first: &super::Fingerprint, second: &super::Fingerprint) -> bool {
    first.size == second.size && first.last_modified_ns == second.last_modified_ns
}

struct Walk<'a> {
    inner: &'a Inner,
    resume: Option<Vec<Vec<u8>>>,
    limit: usize,
    out: Vec<ObjectMeta>,
    incomplete: bool,
    more: bool,
    /// Do not descend: list the files and links of one directory only (the
    /// `""` scope of a scoped listing).
    shallow: bool,
    /// Rules files found on the way down (`.gitignore`, `.ooignore`) above the directory being walked.
    stack: IgnoreStack,
}

impl Walk<'_> {
    fn full(&self) -> bool {
        self.out.len() > self.limit
    }

    fn dir(&mut self, abs: &Path, rel: &mut Vec<Vec<u8>>) {
        if rel.len() >= MAX_DEPTH {
            self.incomplete = true;
            return;
        }
        let Ok(rd) = std::fs::read_dir(abs) else {
            self.incomplete = true;
            return;
        };
        let mut names: Vec<(OsString, bool)> = Vec::new();
        let mut has_rules = false;
        for entry in rd {
            match entry {
                Ok(e) => {
                    let name = e.file_name();
                    has_rules |= name == ".gitignore" || name == ".ooignore";
                    let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
                    names.push((name, is_dir));
                }
                Err(_) => self.incomplete = true,
            }
        }
        names.sort_by_key(|(n, _)| raw_bytes(n));
        let outer = if has_rules {
            let next = self.stack.with(rel.len(), load_dir_rules(abs));
            Some(std::mem::replace(&mut self.stack, next))
        } else {
            None
        };
        for (name, is_dir) in names {
            if self.full() {
                self.more = true;
                break;
            }
            let bytes = raw_bytes(&name);
            if skip_reason_fs(&String::from_utf8_lossy(&bytes), is_dir).is_some() {
                continue;
            }
            if self.ignored_by_rules(rel, &bytes, is_dir) {
                continue;
            }
            rel.push(bytes);
            self.entry(abs, &name, rel);
            rel.pop();
        }
        if let Some(outer) = outer {
            self.stack = outer;
        }
    }

    /// True when a `.gitignore` or `.ooignore` above the entry (named `name`, below the directory `rel`) ignores it.
    fn ignored_by_rules(&self, rel: &[Vec<u8>], name: &[u8], is_dir: bool) -> bool {
        if self.stack.is_empty() {
            return false;
        }
        let mut parts: Vec<String> = rel
            .iter()
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        parts.push(String::from_utf8_lossy(name).into_owned());
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        self.stack.ignored(&refs, is_dir).is_some()
    }

    fn entry(&mut self, abs: &Path, name: &OsStr, rel: &mut Vec<Vec<u8>>) {
        let path = abs.join(name);
        let Ok(md) = std::fs::symlink_metadata(&path) else {
            self.incomplete = true;
            return;
        };
        let kind = kind_of(&md);
        if kind == EntryKind::Dir {
            if self.shallow {
                return;
            }
            if let Some(r) = &self.resume
                && !r.starts_with(rel.as_slice())
                && rel.as_slice() < r.as_slice()
            {
                return;
            }
            self.dir(&path, rel);
            return;
        }
        if kind == EntryKind::Other {
            self.inner.skipped_special.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if let Some(r) = &self.resume
            && rel.as_slice() <= r.as_slice()
        {
            return;
        }
        let key = rel.join(&b'/');
        let meta = meta_of(key, &md);
        if let Some(p) = &meta.path
            && self.inner.excludes.excludes(p.as_str())
        {
            return;
        }
        self.out.push(meta);
    }
}

fn encode_cursor(incomplete: bool, key: &[u8]) -> ListCursor {
    let mut s = String::with_capacity(key.len() * 2 + 1);
    s.push(if incomplete { '!' } else { '=' });
    for b in key {
        s.push_str(&format!("{b:02x}"));
    }
    ListCursor(s)
}

fn decode_cursor(c: &ListCursor) -> Result<(bool, Vec<u8>), DocSourceError> {
    let bad = DocSourceError::Protocol("invalid list cursor");
    let mut chars = c.0.chars();
    let incomplete = match chars.next() {
        Some('!') => true,
        Some('=') => false,
        _ => return Err(bad),
    };
    let hex = chars.as_str().as_bytes();
    if !hex.len().is_multiple_of(2) {
        return Err(bad);
    }
    let mut key = Vec::with_capacity(hex.len() / 2);
    for pair in hex.chunks(2) {
        let s = std::str::from_utf8(pair)
            .map_err(|_| DocSourceError::Protocol("invalid list cursor"))?;
        key.push(
            u8::from_str_radix(s, 16)
                .map_err(|_| DocSourceError::Protocol("invalid list cursor"))?,
        );
    }
    Ok((incomplete, key))
}

impl Inner {
    fn list(
        &self,
        cursor: Option<&ListCursor>,
        page_size: u32,
    ) -> Result<ListPage, DocSourceError> {
        if !std::fs::metadata(&self.root).is_ok_and(|m| m.is_dir()) {
            return Err(root_missing());
        }
        let (carried_incomplete, resume) = match cursor {
            Some(c) => {
                let (i, key) = decode_cursor(c)?;
                (
                    i,
                    Some(key.split(|b| *b == b'/').map(<[u8]>::to_vec).collect()),
                )
            }
            None => (false, None),
        };
        let limit = page_size.clamp(1, MAX_PAGE) as usize;
        let mut walk = Walk {
            inner: self,
            resume,
            limit,
            out: Vec::new(),
            incomplete: carried_incomplete,
            more: false,
            shallow: false,
            stack: IgnoreStack::default(),
        };
        walk.dir(&self.root, &mut Vec::new());
        Ok(finish_page(walk, limit))
    }

    /// First level scopes (10 11.3): every top level directory, plus `""` when
    /// files or links sit directly under the root. `None` when a top level
    /// name is not UTF-8 (the Drive is then listed whole).
    fn list_scopes(&self) -> Result<Option<Vec<String>>, DocSourceError> {
        let rd = std::fs::read_dir(&self.root).map_err(|_| root_missing())?;
        let mut dirs: Vec<String> = Vec::new();
        let mut root_files = false;
        let rules = IgnoreStack::default().with(0, load_dir_rules(&self.root));
        for entry in rd {
            let entry = entry.map_err(|_| DocSourceError::Io("root listing failed"))?;
            let name = entry.file_name();
            let Some(text) = name.to_str() else {
                return Ok(None);
            };
            if skip_reason(text).is_some() {
                continue;
            }
            let md = std::fs::symlink_metadata(self.root.join(&name))
                .map_err(|_| DocSourceError::Io("root listing failed"))?;
            let kind = kind_of(&md);
            if skip_reason_fs(text, kind == EntryKind::Dir).is_some()
                || rules.ignored(&[text], kind == EntryKind::Dir).is_some()
            {
                continue;
            }
            match kind {
                EntryKind::Dir => dirs.push(text.to_owned()),
                EntryKind::File | EntryKind::Symlink => root_files = true,
                EntryKind::Other => {}
            }
        }
        dirs.sort();
        if root_files {
            dirs.insert(0, String::new());
        }
        Ok(Some(dirs))
    }

    /// One page of one scope, same cursor rules as [`Inner::list`].
    fn list_scope(
        &self,
        scope: &str,
        cursor: Option<&ListCursor>,
        page_size: u32,
    ) -> Result<ListPage, DocSourceError> {
        if scope.contains('/') || scope == "." || scope == ".." {
            return Err(DocSourceError::InvalidConfig(
                "scope is not a first level name",
            ));
        }
        if !std::fs::metadata(&self.root).is_ok_and(|m| m.is_dir()) {
            return Err(root_missing());
        }
        let (carried_incomplete, resume) = match cursor {
            Some(c) => {
                let (i, key) = decode_cursor(c)?;
                (
                    i,
                    Some(key.split(|b| *b == b'/').map(<[u8]>::to_vec).collect()),
                )
            }
            None => (false, None),
        };
        let limit = page_size.clamp(1, MAX_PAGE) as usize;
        let mut walk = Walk {
            inner: self,
            resume,
            limit,
            out: Vec::new(),
            incomplete: carried_incomplete,
            more: false,
            shallow: scope.is_empty(),
            stack: IgnoreStack::default().with(0, load_dir_rules(&self.root)),
        };
        if scope.is_empty() {
            walk.stack = IgnoreStack::default();
            walk.dir(&self.root, &mut Vec::new());
        } else {
            let name = OsStr::new(scope);
            // A scope that has vanished lists as nothing: its files are gone.
            if std::fs::symlink_metadata(self.root.join(name))
                .is_ok_and(|m| kind_of(&m) == EntryKind::Dir)
                && skip_reason_fs(scope, true).is_none()
                && !walk.ignored_by_rules(&[], scope.as_bytes(), true)
            {
                walk.entry(&self.root, name, &mut vec![scope.as_bytes().to_vec()]);
            }
        }
        Ok(finish_page(walk, limit))
    }
}

fn finish_page(mut walk: Walk<'_>, limit: usize) -> ListPage {
    let more = walk.out.len() > limit || walk.more;
    walk.out.truncate(limit);
    let incomplete = walk.incomplete;
    let next = if more {
        walk.out
            .last()
            .map(|m| encode_cursor(incomplete, &m.raw_key))
    } else {
        None
    };
    ListPage {
        entries: walk.out,
        complete: next.is_none() && !incomplete,
        next,
    }
}

impl FsSource {
    /// First level scopes of the root, see [`Inner::list_scopes`].
    pub async fn list_scopes(&self) -> Result<Option<Vec<String>>, DocSourceError> {
        let inner = self.inner.clone();
        run_blocking(move || inner.list_scopes()).await
    }

    /// One page of one first level scope (`""` = files directly under the root).
    pub async fn list_scope(
        &self,
        scope: &str,
        cursor: Option<&ListCursor>,
        page_size: u32,
    ) -> Result<ListPage, DocSourceError> {
        let inner = self.inner.clone();
        let scope = scope.to_owned();
        let cursor = cursor.cloned();
        run_blocking(move || inner.list_scope(&scope, cursor.as_ref(), page_size)).await
    }
}

impl DocumentSource for FsSource {
    fn backend(&self) -> DocBackend {
        DocBackend::Fs
    }

    fn capabilities(&self) -> BTreeSet<DocCapability> {
        [
            DocCapability::List,
            DocCapability::Stat,
            DocCapability::RangedRead,
        ]
        .into_iter()
        .collect()
    }

    async fn list(
        &self,
        cursor: Option<&ListCursor>,
        page_size: u32,
    ) -> Result<ListPage, DocSourceError> {
        let inner = self.inner.clone();
        let cursor = cursor.cloned();
        run_blocking(move || inner.list(cursor.as_ref(), page_size)).await
    }

    async fn stat(&self, raw_key: &[u8]) -> Result<Option<ObjectMeta>, DocSourceError> {
        let inner = self.inner.clone();
        let key = raw_key.to_vec();
        run_blocking(move || inner.stat(&key)).await
    }

    async fn read(
        &self,
        raw_key: &[u8],
        offset: u64,
        len: u64,
        expect: &ReadExpect,
    ) -> Result<Vec<u8>, DocSourceError> {
        let inner = self.inner.clone();
        let key = raw_key.to_vec();
        let expect = expect.clone();
        run_blocking(move || inner.read(&key, offset, len, &expect)).await
    }
}
