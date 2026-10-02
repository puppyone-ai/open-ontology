//! Pinned worker image.
//!
//! The trust root is the caller-supplied SHA-256, not the path and not the
//! worker's nonce. Bytes are read through a descriptor opened with `O_NOFOLLOW`
//! on every path component, hashed, and copied into a sealed memfd. The
//! supervisor execs that memfd via `/proc/self/fd`. A path replace after the
//! open does not change the executed bytes.
//!
//! This is not a signature. A caller that hashes an untrusted path and passes
//! the digest has not created an external root. The nonce binds a session to
//! the process that was spawned. It does not authenticate worker output.
//! Symlink resolution, a memfd exec blocked by `memfd_noexec`, and a supervisor
//! that dies before it reaps are outside what this check proves.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::{Component, Path};

use crate::limits::MAX_WORKER_BYTES;
use crate::sha256::sha256;

pub enum ImageError {
    /// Symlink, relative path, hash mismatch, non-regular file, or oversize image.
    Untrusted,
    /// Missing path, directory, or a local OS failure before exec.
    Spawn(io::Error),
}

pub fn sealed_pinned(path: &Path, pin: &[u8; 32]) -> Result<OwnedFd, ImageError> {
    let bytes = read_nofollow(path)?;
    if sha256(&bytes) != *pin {
        return Err(ImageError::Untrusted);
    }
    seal_memfd(&bytes)
}

/// Regular file, nofollow, under the size cap. The digest is not a trust pin.
#[cfg(feature = "test-hooks")]
pub fn sealed_unpinned(path: &Path) -> Result<OwnedFd, ImageError> {
    let bytes = read_nofollow(path)?;
    seal_memfd(&bytes)
}

fn read_nofollow(path: &Path) -> Result<Vec<u8>, ImageError> {
    if !path.is_absolute() {
        return Err(ImageError::Untrusted);
    }
    let mut file = open_nofollow(path)?;
    let meta = file.metadata().map_err(ImageError::Spawn)?;
    let kind = meta.file_type();
    if kind.is_dir() {
        return Err(ImageError::Spawn(io::Error::other("directory")));
    }
    // O_NOFOLLOW already rejected a symlink. Anything else that is not a regular file is untrusted.
    if !kind.is_file() {
        return Err(ImageError::Untrusted);
    }
    if meta.len() == 0 || meta.len() > MAX_WORKER_BYTES {
        return Err(ImageError::Untrusted);
    }
    let mut bytes = Vec::new();
    let mut tmp = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut tmp).map_err(ImageError::Spawn)?;
        if n == 0 {
            break;
        }
        if bytes.len() + n > MAX_WORKER_BYTES as usize {
            return Err(ImageError::Untrusted);
        }
        bytes.extend_from_slice(&tmp[..n]);
    }
    if bytes.is_empty() {
        return Err(ImageError::Untrusted);
    }
    Ok(bytes)
}

fn open_nofollow(path: &Path) -> Result<File, ImageError> {
    // SAFETY: open/openat/close on a path the caller named. O_NOFOLLOW is applied
    // to every component so a symlink is ELOOP instead of a followed target.
    // The returned fd owns the last successful descriptor.
    let mut current = unsafe { libc::open(c"/".as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
    if current < 0 {
        return Err(ImageError::Spawn(io::Error::last_os_error()));
    }
    let parts: Vec<Component> = path.components().collect();
    for (index, component) in parts.iter().enumerate().skip(1) {
        let Component::Normal(name) = component else {
            unsafe { libc::close(current) };
            return Err(ImageError::Untrusted);
        };
        let Ok(c_name) = std::ffi::CString::new(name.as_encoded_bytes()) else {
            unsafe { libc::close(current) };
            return Err(ImageError::Untrusted);
        };
        let last = index + 1 == parts.len();
        let flags = if last {
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC
        } else {
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC
        };
        let next = unsafe { libc::openat(current, c_name.as_ptr(), flags) };
        if next < 0 {
            let error = io::Error::last_os_error();
            // An intermediate symlink can be ELOOP or ENOTDIR depending on
            // O_DIRECTORY. A symlink at any component is an untrusted path,
            // including an ancestor of the final file.
            let untrusted = error.raw_os_error() == Some(libc::ELOOP) || symlink_at(current, c_name.as_ptr());
            unsafe { libc::close(current) };
            return Err(if untrusted { ImageError::Untrusted } else { map_open_error(error) });
        }
        unsafe { libc::close(current) };
        current = next;
    }
    Ok(unsafe { File::from_raw_fd(current) })
}

fn symlink_at(dir: i32, name: *const libc::c_char) -> bool {
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    let rc = unsafe { libc::fstatat(dir, name, &mut stat, libc::AT_SYMLINK_NOFOLLOW) };
    rc == 0 && (stat.st_mode & libc::S_IFMT) == libc::S_IFLNK
}

fn map_open_error(error: io::Error) -> ImageError {
    if error.raw_os_error() == Some(libc::ELOOP) {
        ImageError::Untrusted
    } else {
        ImageError::Spawn(error)
    }
}

fn seal_memfd(bytes: &[u8]) -> Result<OwnedFd, ImageError> {
    let name = std::ffi::CString::new("oo-p05-worker").map_err(|_| ImageError::Untrusted)?;
    // SAFETY: memfd_create returns a fresh fd, which we own. Seals are applied
    // to that fd only after the full image is written. F_SEAL_SEAL freezes the
    // seal set. A failed seal is a spawn failure, not a trusted image.
    let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if fd < 0 {
        return Err(ImageError::Spawn(io::Error::last_os_error()));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    if let Err(error) = file.write_all(bytes) {
        return Err(ImageError::Spawn(error));
    }
    let seals = libc::F_SEAL_WRITE | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_SEAL;
    let rc = unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, seals) };
    if rc < 0 {
        return Err(ImageError::Spawn(io::Error::last_os_error()));
    }
    Ok(file.into())
}
