//! Credential and SQL reads.
//!
//! Token, SQL, and CA paths are opened with `O_NONBLOCK` and `O_NOFOLLOW`,
//! then `fstat` on that fd. `O_NOFOLLOW` alone still waits for a FIFO writer.
//! `O_NONBLOCK` makes that open return. A FIFO is rejected as not a regular
//! file before any byte is read. This does not make every device `open` return.
//!
//! The owner is compared with a probe file this process creates in the temp
//! directory, because `getuid` is unsafe. It does not reject a symlink in a
//! parent directory, an ACL, or a later write by the same owner. The bytes
//! are not wiped when the `String` is dropped.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use oo_core::limits::{DOCUMENT_MAX_BYTES, QUERY_SQL_MAX_BYTES};

use crate::outcome::Failure;

const TOKEN_FILE_MAX: u64 = 4_097;

pub enum ReadError {
    Missing,
    Permission,
    Symlink,
    NotFile,
    Owner,
    Mode,
    Budget,
    Utf8,
    Empty,
    Other,
}

pub fn read_token_file(path: &str) -> Result<String, Failure> {
    match read_token(Path::new(path)) {
        Ok(token) => Ok(token),
        Err(error) => Err(Failure::auth(token_reason(error))),
    }
}

pub fn read_token_stdin() -> Result<String, Failure> {
    let mut buf = Vec::new();
    match io::stdin().take(TOKEN_FILE_MAX + 1).read_to_end(&mut buf) {
        Ok(_) if buf.len() as u64 > TOKEN_FILE_MAX => {
            Err(Failure::auth("token file rejected: budget"))
        }
        Ok(_) => token_bytes(&buf).map_err(|error| Failure::auth(token_reason(error))),
        Err(_) => Err(Failure::auth("token file rejected: other")),
    }
}

pub fn read_sql_file(path: &str) -> Result<String, Failure> {
    match read_capped(
        Path::new(path),
        u64::from(QUERY_SQL_MAX_BYTES as u32),
        false,
    ) {
        Ok(bytes) => match std::str::from_utf8(&bytes) {
            Ok("") => Err(Failure::usage("sql is missing")),
            Ok(text) => Ok(text.to_owned()),
            Err(_) => Err(Failure::local("sql file rejected: utf8")),
        },
        Err(ReadError::Budget) => Err(Failure::local("sql file rejected: budget")),
        Err(error) => Err(Failure::local(sql_reason(error))),
    }
}

pub fn read_ca_file(path: &str) -> Result<Vec<u8>, Failure> {
    match read_capped(Path::new(path), DOCUMENT_MAX_BYTES as u64, false) {
        Ok(bytes) if bytes.is_empty() => Err(Failure::local("ca file rejected: empty")),
        Ok(bytes) => Ok(bytes),
        Err(error) => Err(Failure::local(ca_reason(error))),
    }
}

pub fn read_client_pem(path: &str) -> Result<Vec<u8>, Failure> {
    match read_capped(Path::new(path), DOCUMENT_MAX_BYTES as u64, false) {
        Ok(bytes) if bytes.is_empty() => Err(Failure::local("client pem rejected: empty")),
        Ok(bytes) if bytes.contains(&0) => Err(Failure::local("client pem rejected: other")),
        Ok(bytes) => Ok(bytes),
        Err(error) => Err(Failure::local(client_pem_reason(error))),
    }
}

fn read_token(path: &Path) -> Result<String, ReadError> {
    let bytes = read_capped(path, TOKEN_FILE_MAX, true)?;
    token_bytes(&bytes)
}

fn token_bytes(bytes: &[u8]) -> Result<String, ReadError> {
    if bytes.len() as u64 > TOKEN_FILE_MAX {
        return Err(ReadError::Budget);
    }
    let mut text = std::str::from_utf8(bytes).map_err(|_| ReadError::Utf8)?;
    if let Some(stripped) = text.strip_suffix('\n') {
        text = stripped;
    }
    if text.is_empty() {
        return Err(ReadError::Empty);
    }
    if text.len() > 4_096 || text.bytes().any(|byte| !(0x21..=0x7e).contains(&byte)) {
        return Err(ReadError::Utf8);
    }
    Ok(text.to_owned())
}

fn read_capped(path: &Path, cap: u64, token_policy: bool) -> Result<Vec<u8>, ReadError> {
    let mut options = OpenOptions::new();
    options.read(true);
    // Both flags are on this open. A path stat first would race, and
    // `O_NOFOLLOW` alone still blocks on a peerless FIFO.
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options.open(path).map_err(map_open)?;
    inspect(&file, cap, token_policy)?;
    let mut buf = Vec::new();
    file.take(cap.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|_| ReadError::Other)?;
    if buf.len() as u64 > cap {
        return Err(ReadError::Budget);
    }
    Ok(buf)
}

fn inspect(file: &File, cap: u64, token_policy: bool) -> Result<(), ReadError> {
    let meta = file.metadata().map_err(|_| ReadError::Other)?;
    if !meta.is_file() {
        return Err(ReadError::NotFile);
    }
    if meta.len() > cap {
        return Err(ReadError::Budget);
    }
    // A mode-000 file is not granted to its owner. CAP_DAC_OVERRIDE still
    // lets this process open it; reject that before any byte is read.
    if !token_policy && meta.mode() & 0o777 == 0 {
        return Err(ReadError::Permission);
    }
    if token_policy {
        let me = process_owner()?;
        if meta.uid() != me {
            return Err(ReadError::Owner);
        }
        if meta.mode() & 0o7777 != 0o600 {
            return Err(ReadError::Mode);
        }
    }
    Ok(())
}

/// Owner of a regular file this process just created.
///
/// `getuid` is an unsafe libc call, and this crate forbids unsafe code. The
/// uid on the new inode is the effective uid. The probe is unlinked before
/// the token bytes are read. A temp directory that rejects the create fails
/// the token read.
fn process_owner() -> Result<u32, ReadError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(".oo-cli-owner-{}-{n}", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options.custom_flags(libc::O_NOFOLLOW);
    let file = options.open(&path).map_err(|_| ReadError::Other)?;
    let uid = file.metadata().map_err(|_| ReadError::Other)?.uid();
    drop(file);
    let _ = std::fs::remove_file(&path);
    Ok(uid)
}

fn map_open(error: io::Error) -> ReadError {
    if error.kind() == io::ErrorKind::NotFound {
        ReadError::Missing
    } else if error.kind() == io::ErrorKind::PermissionDenied {
        ReadError::Permission
    } else if error.raw_os_error() == Some(libc::ELOOP) {
        ReadError::Symlink
    } else {
        ReadError::Other
    }
}

fn token_reason(error: ReadError) -> &'static str {
    match error {
        ReadError::Missing => "token file rejected: missing",
        ReadError::Permission => "token file rejected: permission",
        ReadError::Symlink => "token file rejected: symlink",
        ReadError::NotFile => "token file rejected: not-file",
        ReadError::Owner => "token file rejected: owner",
        ReadError::Mode => "token file rejected: mode",
        ReadError::Budget => "token file rejected: budget",
        ReadError::Utf8 | ReadError::Empty => "token rejected",
        ReadError::Other => "token file rejected: other",
    }
}

fn sql_reason(error: ReadError) -> &'static str {
    match error {
        ReadError::Missing => "sql file rejected: missing",
        ReadError::Permission => "sql file rejected: permission",
        ReadError::Symlink => "sql file rejected: symlink",
        ReadError::NotFile => "sql file rejected: not-file",
        ReadError::Budget => "sql file rejected: budget",
        ReadError::Utf8 => "sql file rejected: utf8",
        ReadError::Empty => "sql is missing",
        ReadError::Owner | ReadError::Mode | ReadError::Other => "sql file rejected: other",
    }
}

fn client_pem_reason(error: ReadError) -> &'static str {
    match error {
        ReadError::Missing => "client pem rejected: missing",
        ReadError::Permission => "client pem rejected: permission",
        ReadError::Symlink => "client pem rejected: symlink",
        ReadError::NotFile => "client pem rejected: not-file",
        ReadError::Budget => "client pem rejected: budget",
        ReadError::Empty => "client pem rejected: empty",
        ReadError::Utf8 | ReadError::Owner | ReadError::Mode | ReadError::Other => {
            "client pem rejected: other"
        }
    }
}

fn ca_reason(error: ReadError) -> &'static str {
    match error {
        ReadError::Missing => "ca file rejected: missing",
        ReadError::Permission => "ca file rejected: permission",
        ReadError::Symlink => "ca file rejected: symlink",
        ReadError::NotFile => "ca file rejected: not-file",
        ReadError::Budget => "ca file rejected: budget",
        ReadError::Empty => "ca file rejected: empty",
        ReadError::Utf8 | ReadError::Owner | ReadError::Mode | ReadError::Other => {
            "ca file rejected: other"
        }
    }
}
