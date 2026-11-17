//! Content-addressed store (10 §3.3). Board WS-BE1.
//!
//! Layout under `<state>/cas/`: `VERSION`, `orig/`, `ir/`, `llm/`, `parts/`,
//! `packs/` and `tmp/`. Hash-addressed kinds are in [`layout::CasKind`];
//! `parts/` and `packs/` belong to the resume and compile cards and are only
//! created here.
//!
//! Writes go to `tmp/` with `O_EXCL` 0600, are fsynced, renamed to the
//! content address and the target directory is fsynced, so a crash leaves
//! either no object or a complete one. Reads re-hash and delete a corrupt
//! object (`DOCS_ENGINE_CAS_CORRUPT`); the caller requeues the jobs that
//! referenced it.

pub mod gc;
pub mod layout;
#[cfg(kani)]
mod proofs;
#[cfg(test)]
mod tests;

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

pub use gc::{EvictReport, GcReport, RefSet};
pub use layout::{CasKind, is_b3_hex, object_rel_path};

/// Contents of `VERSION`.
pub const CAS_VERSION: &str = "oo.docs.cas/v1";
const SUBDIRS: [&str; 6] = ["orig/b3", "ir", "llm", "parts", "packs", "tmp"];

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CasError {
    /// The stored bytes do not hash to their address; the object was deleted.
    #[error("CAS object failed hash verification")]
    Corrupt { kind: CasKind, b3: String },
    #[error("CAS key is not 64 lowercase hex characters")]
    BadKey,
    #[error("CAS object not found")]
    NotFound,
    #[error("CAS directory has an unsupported VERSION")]
    VersionMismatch,
    #[error("CAS I/O failure: {0}")]
    Io(&'static str),
}

impl CasError {
    /// Stable machine code (10 §8.4).
    pub fn code(&self) -> &'static str {
        match self {
            CasError::Corrupt { .. } => "DOCS_ENGINE_CAS_CORRUPT",
            CasError::BadKey | CasError::VersionMismatch => "DOCS_ENGINE_CAS_INVALID",
            CasError::NotFound => "DOCS_ENGINE_CAS_MISSING",
            CasError::Io(_) => "DOCS_ENGINE_CAS_IO",
        }
    }
}

fn io(what: &'static str) -> impl FnOnce(std::io::Error) -> CasError {
    move |_| CasError::Io(what)
}

/// Result of a write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Put {
    pub b3: String,
    pub size: u64,
    /// `false` when an identical object already existed.
    pub created: bool,
    /// `true` when an existing object at the address was corrupt and replaced.
    pub repaired: bool,
}

/// Where a simulated crash stops a write. Only tests set it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum CrashAt {
    /// After writing this many bytes to `tmp/`, nothing synced.
    MidWrite(u64),
    /// Temp file complete and fsynced, not yet renamed.
    BeforeRename,
}

#[derive(Debug)]
pub struct Cas {
    pub(crate) root: PathBuf,
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

impl Cas {
    /// Create or open the store, check `VERSION` and clear `tmp/`.
    pub fn open(root: &Path) -> Result<Self, CasError> {
        mkdir_all(root)?;
        for sub in SUBDIRS {
            mkdir_all(&root.join(sub))?;
        }
        let version = root.join("VERSION");
        match fs::read_to_string(&version) {
            Ok(v) if v.trim_end() == CAS_VERSION => {}
            Ok(_) => return Err(CasError::VersionMismatch),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                write_file_atomic(root, &version, format!("{CAS_VERSION}\n").as_bytes())?;
            }
            Err(_) => return Err(CasError::Io("cannot read VERSION")),
        }
        let cas = Self {
            root: root.to_path_buf(),
        };
        cas.clear_tmp()?;
        Ok(cas)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn clear_tmp(&self) -> Result<(), CasError> {
        let tmp = self.root.join("tmp");
        for entry in fs::read_dir(&tmp).map_err(io("cannot list tmp"))? {
            let path = entry.map_err(io("cannot list tmp"))?.path();
            let _ = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
        }
        Ok(())
    }

    fn abs(&self, kind: CasKind, b3: &str) -> Result<PathBuf, CasError> {
        let rel = object_rel_path(kind, b3).ok_or(CasError::BadKey)?;
        Ok(self.root.join(rel))
    }

    /// Write bytes; the address is their BLAKE3.
    pub fn put(&self, kind: CasKind, bytes: &[u8]) -> Result<Put, CasError> {
        self.put_reader(kind, bytes)
    }

    /// Write many small objects of one kind without one fsync round trip per
    /// object. With `durable` every object and its directory are fsynced (by a
    /// few threads) before this returns, the guarantee of [`Self::put`]. Without
    /// it nothing is fsynced: for derived objects that can be rebuilt (packs),
    /// where the full fsync of a laptop disk would cost seconds per compile. An
    /// object that is there with the right size is left alone; one with the
    /// wrong size (a torn write after a crash) is written again. Returns the
    /// BLAKE3 of each item, in order.
    pub fn put_batch(
        &self,
        kind: CasKind,
        items: &[&[u8]],
        durable: bool,
    ) -> Result<Vec<String>, CasError> {
        let mut out = Vec::with_capacity(items.len());
        let mut todo: std::collections::BTreeMap<String, &[u8]> = std::collections::BTreeMap::new();
        for bytes in items {
            let b3 = blake3::hash(bytes).to_hex().to_string();
            let whole = fs::metadata(self.abs(kind, &b3)?)
                .is_ok_and(|m| m.is_file() && m.len() == bytes.len() as u64);
            if !whole {
                todo.insert(b3.clone(), bytes);
            }
            out.push(b3);
        }
        let mut synced: Vec<PathBuf> = Vec::new();
        let mut dirs: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
        for (b3, bytes) in &todo {
            let dest = self.abs(kind, b3)?;
            let parent = dest.parent().ok_or(CasError::Io("object has no parent"))?;
            mkdir_all(parent)?;
            let tmp = self.root.join("tmp").join(format!(
                "{}-{}-{}.part",
                std::process::id(),
                TMP_SEQ.fetch_add(1, Ordering::Relaxed),
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos())
            ));
            let mut f = create_excl(&tmp)?;
            if let Err(e) = f.write_all(bytes) {
                drop(f);
                let _ = fs::remove_file(&tmp);
                return Err(io("cannot write tmp")(e));
            }
            drop(f);
            fs::rename(&tmp, &dest).map_err(io("cannot rename into place"))?;
            dirs.insert(parent.to_path_buf());
            synced.push(dest);
        }
        if !durable {
            return Ok(out);
        }
        synced.extend(dirs);
        let per = synced.len().div_ceil(8).max(1);
        let failed = std::thread::scope(|s| {
            let handles: Vec<_> = synced
                .chunks(per)
                .map(|chunk| {
                    s.spawn(move || {
                        chunk
                            .iter()
                            .any(|p| File::open(p).and_then(|f| f.sync_all()).is_err())
                    })
                })
                .collect();
            handles.into_iter().any(|h| h.join().unwrap_or(true))
        });
        if failed {
            return Err(CasError::Io("cannot fsync batch"));
        }
        Ok(out)
    }

    /// Stream a reader into the store (originals can be large).
    pub fn put_reader<R: Read>(&self, kind: CasKind, reader: R) -> Result<Put, CasError> {
        self.write_object(kind, reader, None)
            .map(|p| p.expect("no crash requested"))
    }

    pub(crate) fn write_object<R: Read>(
        &self,
        kind: CasKind,
        mut reader: R,
        crash: Option<CrashAt>,
    ) -> Result<Option<Put>, CasError> {
        let tmp = self.root.join("tmp").join(format!(
            "{}-{}-{}.part",
            std::process::id(),
            TMP_SEQ.fetch_add(1, Ordering::Relaxed),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        let mut file = create_excl(&tmp)?;
        let mut hasher = blake3::Hasher::new();
        let mut size = 0u64;
        let mut buf = vec![0u8; 64 * 1024];
        let outcome = (|| -> Result<(), CasError> {
            loop {
                let n = reader.read(&mut buf).map_err(io("cannot read input"))?;
                if n == 0 {
                    return Ok(());
                }
                let mut chunk = &buf[..n];
                if let Some(CrashAt::MidWrite(limit)) = crash {
                    let room = limit.saturating_sub(size) as usize;
                    chunk = &chunk[..chunk.len().min(room)];
                    file.write_all(chunk).map_err(io("cannot write tmp"))?;
                    size += chunk.len() as u64;
                    if size >= limit {
                        return Err(CasError::Io("simulated crash"));
                    }
                    continue;
                }
                hasher.update(chunk);
                file.write_all(chunk).map_err(io("cannot write tmp"))?;
                size += chunk.len() as u64;
            }
        })();
        if let Err(e) = outcome {
            if crash.is_none() {
                drop(file);
                let _ = fs::remove_file(&tmp);
            }
            return if crash.is_some() { Ok(None) } else { Err(e) };
        }
        file.sync_all().map_err(io("cannot fsync tmp"))?;
        drop(file);
        if crash == Some(CrashAt::BeforeRename) {
            return Ok(None);
        }
        let b3 = hasher.finalize().to_hex().to_string();
        let dest = self.abs(kind, &b3)?;
        let (created, repaired) = match fs::metadata(&dest) {
            Ok(_) if file_b3(&dest).is_ok_and(|h| h == b3) => {
                let _ = fs::remove_file(&tmp);
                (false, false)
            }
            Ok(_) => {
                rename_into(&tmp, &dest)?;
                (true, true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                rename_into(&tmp, &dest)?;
                (true, false)
            }
            Err(_) => {
                let _ = fs::remove_file(&tmp);
                return Err(CasError::Io("cannot stat target"));
            }
        };
        Ok(Some(Put {
            b3,
            size,
            created,
            repaired,
        }))
    }

    pub fn exists(&self, kind: CasKind, b3: &str) -> Result<bool, CasError> {
        Ok(self.abs(kind, b3)?.is_file())
    }

    /// Read and verify. A mismatch deletes the object and returns
    /// [`CasError::Corrupt`].
    pub fn get(&self, kind: CasKind, b3: &str) -> Result<Vec<u8>, CasError> {
        let path = self.abs(kind, b3)?;
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(CasError::NotFound),
            Err(_) => return Err(CasError::Io("cannot read object")),
        };
        if blake3::hash(&bytes).to_hex().as_str() != b3 {
            let _ = fs::remove_file(&path);
            return Err(CasError::Corrupt {
                kind,
                b3: b3.to_owned(),
            });
        }
        if kind == CasKind::Orig {
            touch(&path);
        }
        Ok(bytes)
    }

    /// Streaming verification (the `cas_sample` doctor check): re-hash
    /// without loading the object; delete on mismatch.
    pub fn verify(&self, kind: CasKind, b3: &str) -> Result<(), CasError> {
        let path = self.abs(kind, b3)?;
        match file_b3(&path) {
            Ok(h) if h == b3 => Ok(()),
            Ok(_) => {
                let _ = fs::remove_file(&path);
                Err(CasError::Corrupt {
                    kind,
                    b3: b3.to_owned(),
                })
            }
            Err(CasError::NotFound) => Err(CasError::NotFound),
            Err(e) => Err(e),
        }
    }

    /// Remove one object (used by eviction). Missing is fine.
    pub fn remove(&self, kind: CasKind, b3: &str) -> Result<bool, CasError> {
        match fs::remove_file(self.abs(kind, b3)?) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(CasError::Io("cannot remove object")),
        }
    }
}

pub(crate) fn touch(path: &Path) {
    if let Ok(f) = File::options().read(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

/// Stream-hash a file.
pub(crate) fn file_b3(path: &Path) -> Result<String, CasError> {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(CasError::NotFound),
        Err(_) => return Err(CasError::Io("cannot open object")),
    };
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(io("cannot read object"))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}

#[cfg(unix)]
fn mkdir_all(path: &Path) -> Result<(), CasError> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(io("cannot create directory"))
}

#[cfg(not(unix))]
fn mkdir_all(path: &Path) -> Result<(), CasError> {
    fs::create_dir_all(path).map_err(io("cannot create directory"))
}

#[cfg(unix)]
fn create_excl(path: &Path) -> Result<File, CasError> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(io("cannot create tmp file"))
}

#[cfg(not(unix))]
fn create_excl(path: &Path) -> Result<File, CasError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io("cannot create tmp file"))
}

fn fsync_dir(dir: &Path) -> Result<(), CasError> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(io("cannot fsync directory"))
}

/// Rename a finished temp file into place, then fsync the target directory.
fn rename_into(tmp: &Path, dest: &Path) -> Result<(), CasError> {
    let parent = dest.parent().ok_or(CasError::Io("object has no parent"))?;
    mkdir_all(parent)?;
    fs::rename(tmp, dest).map_err(io("cannot rename into place"))?;
    fsync_dir(parent)
}

/// Small file written with the same protocol (VERSION).
fn write_file_atomic(root: &Path, dest: &Path, bytes: &[u8]) -> Result<(), CasError> {
    let tmp = root
        .join("tmp")
        .join(format!("version-{}.part", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let mut f = create_excl(&tmp)?;
    f.write_all(bytes).map_err(io("cannot write tmp"))?;
    f.sync_all().map_err(io("cannot fsync tmp"))?;
    drop(f);
    rename_into(&tmp, dest)
}
