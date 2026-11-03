//! Owner-private secret files. Same fd checks as oo-cli/load.rs, plus parent
//! symlink rejection. Unix only; ACL and same-owner concurrent mutation are
//! outside this file-mode boundary. Secret bytes are not wiped on drop.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::stack::{Result, secret_name};

pub struct SecretStore {
    root: PathBuf,
}

impl SecretStore {
    pub fn from_home() -> Result<Self> {
        let home = std::env::var_os("HOME").ok_or("SECRET_HOME")?;
        Self::open(Path::new(&home).join(".config/open-ontology/secrets"))
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        check_parents(&root)?;
        let store = Self { root };
        store.check()?;
        Ok(store)
    }

    /// The caller creates the non-secret parent configuration directory.
    pub fn create(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        check_parents(root.parent().ok_or("SECRET_PATH")?)?;
        match fs::DirBuilder::new().mode(0o700).create(&root) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err("SECRET_DIRECTORY"),
        }
        Self::open(root)
    }

    fn check(&self) -> Result<()> {
        check_parents(&self.root)?;
        let meta = fs::symlink_metadata(&self.root).map_err(|_| "SECRET_DIRECTORY")?;
        if !meta.is_dir() || meta.mode() & 0o7777 != 0o700 || meta.uid() != process_owner()? {
            return Err("SECRET_DIRECTORY");
        }
        Ok(())
    }

    pub fn read(&self, name: &str) -> Result<String> {
        if !secret_name(name) {
            return Err("SECRET_NAME");
        }
        self.check()?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.root.join(name))
            .map_err(|_| "SECRET_OPEN")?;
        inspect(&file)?;
        let mut bytes = Vec::new();
        file.take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|_| "SECRET_READ")?;
        if bytes.is_empty() || bytes.len() > 65_536 {
            return Err("SECRET_BUDGET");
        }
        let text = String::from_utf8(bytes).map_err(|_| "SECRET_UTF8")?;
        if text.contains('\0') {
            return Err("SECRET_UTF8");
        }
        Ok(text)
    }

    /// Never replace an existing credential implicitly.
    pub fn write_new(&self, name: &str, bytes: &[u8]) -> Result<()> {
        if !secret_name(name) {
            return Err("SECRET_NAME");
        }
        if bytes.is_empty() || bytes.len() > 65_536 {
            return Err("SECRET_BUDGET");
        }
        self.check()?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.root.join(name))
            .map_err(|_| "SECRET_CREATE")?;
        inspect(&file)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "SECRET_WRITE")
    }
}

fn inspect(file: &File) -> Result<()> {
    let meta = file.metadata().map_err(|_| "SECRET_STAT")?;
    if !meta.is_file()
        || meta.mode() & 0o7777 != 0o600
        || meta.uid() != process_owner()?
        || meta.nlink() != 1
        || meta.len() > 65_536
    {
        return Err("SECRET_FILE");
    }
    Ok(())
}

fn check_parents(path: &Path) -> Result<()> {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err("SECRET_PATH");
        }
        prefix.push(component);
        let meta = fs::symlink_metadata(&prefix).map_err(|_| "SECRET_PATH")?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err("SECRET_PATH");
        }
    }
    Ok(())
}

fn process_owner() -> Result<u32> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(".oo-stack-owner-{}-{n}", std::process::id()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|_| "SECRET_OWNER")?;
    let result = file.metadata().map(|m| m.uid()).map_err(|_| "SECRET_OWNER");
    drop(file);
    fs::remove_file(path).map_err(|_| "SECRET_OWNER")?;
    result
}
