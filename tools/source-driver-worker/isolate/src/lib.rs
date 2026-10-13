//! Marks every descriptor above stderr close-on-exec in the forked child.
//!
//! Rust 1.94 `do_exec` dup2s stdio onto fds 0, 1, and 2, runs `pre_exec`, then
//! `execvp`. `close_range(..., CLOSE_RANGE_CLOEXEC)` is one syscall on that
//! child side. It does not close or change flags in the parent. Closing the
//! range instead would drop the exec-status socket Rust still needs if `exec`
//! fails, so the flag is `CLOSE_RANGE_CLOEXEC` (Linux 5.11+, value 4).
//!
//! The supervisor crate keeps `forbid(unsafe_code)`. This crate is the only
//! place that calls the syscall. A missing `close_range` returns `ENOSYS` and
//! the supervisor does not exec.

#![deny(unsafe_code)]

use std::io;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::process::{Child, Command};
use std::os::unix::process::CommandExt;

/// `CLOSE_RANGE_CLOEXEC` from the Linux uapi. Checked against libc before spawn.
const CLOSE_RANGE_CLOEXEC: libc::c_int = 4;

/// Spawn `command` after arranging that exec keeps only fds 0, 1, and 2.
///
/// On a non-Linux target this returns `ENOSYS` and does not spawn.
#[allow(unsafe_code)]
pub fn spawn_isolated(command: &mut Command) -> io::Result<Child> {
    #[cfg(target_os = "linux")]
    {
        debug_assert_eq!(
            libc::CLOSE_RANGE_CLOEXEC as libc::c_int,
            CLOSE_RANGE_CLOEXEC,
            "libc CLOSE_RANGE_CLOEXEC changed"
        );
        // Safety: `mark_cloexec_above_stdio` runs only in the forked child,
        // after stdio dup2 and before exec. The success path is one syscall.
        unsafe {
            command.pre_exec(mark_cloexec_above_stdio);
        }
        command.spawn()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = command;
        Err(io::Error::from_raw_os_error(libc::ENOSYS))
    }
}

/// True when the isolate syscall is absent. The caller must not exec.
pub fn is_missing_isolate(code: Option<i32>) -> bool {
    code == Some(libc::ENOSYS)
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn mark_cloexec_above_stdio() -> io::Result<()> {
    let rc = unsafe { libc::close_range(3, libc::c_uint::MAX, CLOSE_RANGE_CLOEXEC) };
    if rc == 0 {
        Ok(())
    } else {
        let code = io::Error::last_os_error().raw_os_error().unwrap_or(libc::ENOSYS);
        Err(io::Error::from_raw_os_error(code))
    }
}

/// Duplicate `fd` with `F_DUPFD` so the copy has no close-on-exec flag.
///
/// The original descriptor number and flags stay as they were. Tests use this
/// to open a pipe, file, or socket that would have crossed `exec` without
/// isolation. It is not a spawn path.
#[allow(unsafe_code)]
pub fn duplicate_without_cloexec(fd: impl AsFd) -> io::Result<OwnedFd> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = fd;
        return Err(io::Error::from_raw_os_error(libc::ENOSYS));
    }
    #[cfg(target_os = "linux")]
    {
        let raw = fd.as_fd().as_raw_fd();
        let dup = unsafe { libc::fcntl(raw, libc::F_DUPFD, 3) };
        if dup < 0 {
            Err(io::Error::last_os_error())
        } else {
            // Safety: F_DUPFD returned a new descriptor. No other owner has it.
            // The borrowed source descriptor is not closed and its flags are not set.
            Ok(unsafe { OwnedFd::from_raw_fd(dup) })
        }
    }
}
