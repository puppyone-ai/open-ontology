//! Linux development credential boundary.
//!
//! `CommandExt::uid` asks the runtime to drop supplementary groups, but the
//! pinned std implementation ignores `EPERM` from `setgroups`. This module
//! fail-closes unless the broker can perform the drop and `/proc` shows the
//! child actually landed on the requested ids with an empty group list.
//! It is not a runsc launch and it does not claim network containment.

use crate::{Error, ErrorKind};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::Path;

const STATUS_LIMIT: usize = 65_536;
const CAP_CHOWN: u128 = 1 << 0;
const CAP_SETGID: u128 = 1 << 6;
const CAP_SETUID: u128 = 1 << 7;

/// Linux `O_DIRECTORY|O_NOFOLLOW`. The numeric values differ by architecture.
/// aarch64 glibc on the pinned image uses `0x4000` and `0x8000`.
/// `0x10000` is `O_DIRECT` there, and tmpfs rejects it with `EINVAL`.
/// x86_64 and i686 use `0x10000` and `0x20000`. Other architectures fail closed.
#[cfg(target_os = "linux")]
fn directory_nofollow_flags() -> Option<i32> {
    #[cfg(target_arch = "aarch64")]
    {
        Some(0x4000 | 0x8000)
    }
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        Some(0x1_0000 | 0x2_0000)
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64", target_arch = "x86")))]
    {
        None
    }
}

/// Requested child credentials. Construct only with [`DevelopmentUidBoundary::checked`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevelopmentUidBoundary {
    uid: u32,
    gid: u32,
}

/// Public, non-secret process identity captured from `/proc` after spawn.
/// The four id fields are real, effective, saved, and filesystem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UidBoundaryReceipt {
    pub parent_uids: [u32; 4],
    pub parent_gids: [u32; 4],
    pub parent_groups: Vec<u32>,
    pub parent_no_new_privs: u32,
    pub parent_cap_inh: u128,
    pub parent_cap_prm: u128,
    pub parent_cap_eff: u128,
    pub parent_cap_bnd: u128,
    pub parent_cap_amb: u128,
    pub child_uids: [u32; 4],
    pub child_gids: [u32; 4],
    pub child_groups: Vec<u32>,
    pub child_no_new_privs: u32,
    pub child_cap_inh: u128,
    pub child_cap_prm: u128,
    pub child_cap_eff: u128,
    pub child_cap_bnd: u128,
    pub child_cap_amb: u128,
}

#[derive(Clone, Debug)]
struct ProcStatus {
    uids: [u32; 4],
    gids: [u32; 4],
    groups: Vec<u32>,
    no_new_privs: u32,
    cap_inh: u128,
    cap_prm: u128,
    cap_eff: u128,
    cap_bnd: u128,
    cap_amb: u128,
}

impl DevelopmentUidBoundary {
    /// Fail closed before a child exists.
    ///
    /// Order: not Linux, uid 0, gid 0, uid equal to any parent uid, missing
    /// `CAP_CHOWN` / `CAP_SETUID` / `CAP_SETGID` in `CapEff`, then `NoNewPrivs`.
    pub fn checked(uid: u32, gid: u32) -> Result<Self, Error> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (uid, gid);
            return Err(unavailable("platform is not linux"));
        }
        #[cfg(target_os = "linux")]
        {
            validate(uid, gid)?;
            Ok(Self { uid, gid })
        }
    }

    pub(crate) fn recheck(&self) -> Result<(), Error> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = self;
            return Err(unavailable("platform is not linux"));
        }
        #[cfg(target_os = "linux")]
        {
            validate(self.uid, self.gid)
        }
    }

    pub(crate) fn identity(&self) -> crate::child::ChildIdentity {
        crate::child::ChildIdentity::new(self.uid, self.gid)
    }

    /// Change ownership of a directory the caller already prepared.
    ///
    /// Opens with `O_DIRECTORY|O_NOFOLLOW` and `fchown`s that fd. Mode bits stay
    /// as the caller set them. A symlink, a non-directory, or a changed inode fails.
    pub fn assign_directory(&self, path: &Path) -> Result<(), Error> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = path;
            return Err(unavailable("platform is not linux"));
        }
        #[cfg(target_os = "linux")]
        {
            assign_directory_linux(self, path)
        }
    }
}

pub(crate) fn unavailable(reason: &str) -> Error {
    Error::new(ErrorKind::UidBoundaryUnavailable, format!("uid_boundary_unavailable: {reason}"))
}

pub(crate) fn unverified(reason: &str) -> Error {
    Error::new(ErrorKind::UidBoundaryUnverified, format!("uid_boundary_unverified: {reason}"))
}

pub(crate) fn verify_child(pid: u32, boundary: &DevelopmentUidBoundary) -> Result<UidBoundaryReceipt, Error> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (pid, boundary);
        return Err(unverified("platform is not linux"));
    }
    #[cfg(target_os = "linux")]
    {
        verify_child_linux(pid, boundary)
    }
}

#[cfg(target_os = "linux")]
fn validate(uid: u32, gid: u32) -> Result<(), Error> {
    if uid == 0 {
        return Err(unavailable("uid is 0"));
    }
    if gid == 0 {
        return Err(unavailable("gid is 0"));
    }
    let parent = read_proc("self").map_err(|_| unavailable("parent status unreadable"))?;
    if let Err(reason) = require_broker(&parent, uid) {
        return Err(unavailable(reason));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn require_broker(status: &ProcStatus, uid: u32) -> Result<(), &'static str> {
    if status.uids.contains(&uid) {
        return Err("uid equals parent");
    }
    if status.cap_eff & CAP_CHOWN == 0 {
        return Err("cap_chown");
    }
    if status.cap_eff & CAP_SETUID == 0 {
        return Err("cap_setuid");
    }
    if status.cap_eff & CAP_SETGID == 0 {
        return Err("cap_setgid");
    }
    if status.no_new_privs != 1 {
        return Err("no_new_privs");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn verify_child_linux(pid: u32, boundary: &DevelopmentUidBoundary) -> Result<UidBoundaryReceipt, Error> {
    let parent = read_proc("self").map_err(|_| unverified("parent status unreadable"))?;
    if let Err(reason) = require_broker(&parent, boundary.uid) {
        return Err(unverified(reason));
    }
    let child = read_proc(&pid.to_string()).map_err(|_| unverified("child status unreadable"))?;
    if child.uids != [boundary.uid; 4] {
        return Err(unverified("child uid mismatch"));
    }
    if child.gids != [boundary.gid; 4] {
        return Err(unverified("child gid mismatch"));
    }
    if !child.groups.is_empty() {
        return Err(unverified("child groups are not empty"));
    }
    if child.no_new_privs != 1 {
        return Err(unverified("child no_new_privs"));
    }
    if child.cap_eff != 0 {
        return Err(unverified("child cap_eff"));
    }
    if child.cap_prm != 0 {
        return Err(unverified("child cap_prm"));
    }
    if child.cap_inh != 0 {
        return Err(unverified("child cap_inh"));
    }
    if child.cap_amb != 0 {
        return Err(unverified("child cap_amb"));
    }
    Ok(UidBoundaryReceipt {
        parent_uids: parent.uids,
        parent_gids: parent.gids,
        parent_groups: parent.groups,
        parent_no_new_privs: parent.no_new_privs,
        parent_cap_inh: parent.cap_inh,
        parent_cap_prm: parent.cap_prm,
        parent_cap_eff: parent.cap_eff,
        parent_cap_bnd: parent.cap_bnd,
        parent_cap_amb: parent.cap_amb,
        child_uids: child.uids,
        child_gids: child.gids,
        child_groups: child.groups,
        child_no_new_privs: child.no_new_privs,
        child_cap_inh: child.cap_inh,
        child_cap_prm: child.cap_prm,
        child_cap_eff: child.cap_eff,
        child_cap_bnd: child.cap_bnd,
        child_cap_amb: child.cap_amb,
    })
}

#[cfg(target_os = "linux")]
fn assign_directory_linux(boundary: &DevelopmentUidBoundary, path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::{fchown, MetadataExt, OpenOptionsExt};

    if !path.is_absolute() {
        return Err(unavailable("directory chown failed"));
    }
    let Some(flags) = directory_nofollow_flags() else {
        return Err(unavailable("directory chown failed"));
    };
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(path)
        .map_err(|_| unavailable("directory chown failed"))?;
    let before = file.metadata().map_err(|_| unavailable("directory chown failed"))?;
    if !before.is_dir() {
        return Err(unavailable("directory chown failed"));
    }
    let mode = before.mode();
    let dev = before.dev();
    let ino = before.ino();
    fchown(&file, Some(boundary.uid), Some(boundary.gid)).map_err(|_| unavailable("directory chown failed"))?;
    let after = file.metadata().map_err(|_| unverified("directory identity changed"))?;
    if after.dev() != dev || after.ino() != ino || after.mode() != mode {
        return Err(unverified("directory identity changed"));
    }
    if after.uid() != boundary.uid || after.gid() != boundary.gid {
        return Err(unverified("directory owner mismatch"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_proc(pid: &str) -> Result<ProcStatus, ()> {
    if pid != "self" && pid.parse::<u32>().is_err() {
        return Err(());
    }
    let mut file = File::open(format!("/proc/{pid}/status")).map_err(|_| ())?;
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 4096];
    loop {
        let read = file.read(&mut tmp).map_err(|_| ())?;
        if read == 0 {
            break;
        }
        if buf.len().saturating_add(read) > STATUS_LIMIT {
            return Err(());
        }
        buf.extend_from_slice(&tmp[..read]);
    }
    let text = std::str::from_utf8(&buf).map_err(|_| ())?;
    parse_status(text)
}

#[cfg(target_os = "linux")]
fn parse_status(text: &str) -> Result<ProcStatus, ()> {
    const UID: u16 = 1;
    const GID: u16 = 2;
    const GROUPS: u16 = 4;
    const NNP: u16 = 8;
    const INH: u16 = 16;
    const PRM: u16 = 32;
    const EFF: u16 = 64;
    const BND: u16 = 128;
    const AMB: u16 = 256;
    const NEED: u16 = UID | GID | GROUPS | NNP | INH | PRM | EFF | BND | AMB;
    let mut seen = 0_u16;
    let mut status = ProcStatus {
        uids: [0; 4],
        gids: [0; 4],
        groups: Vec::new(),
        no_new_privs: 0,
        cap_inh: 0,
        cap_prm: 0,
        cap_eff: 0,
        cap_bnd: 0,
        cap_amb: 0,
    };
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let rest = rest.trim();
        let bit = match key {
            "Uid" => UID,
            "Gid" => GID,
            "Groups" => GROUPS,
            "NoNewPrivs" => NNP,
            "CapInh" => INH,
            "CapPrm" => PRM,
            "CapEff" => EFF,
            "CapBnd" => BND,
            "CapAmb" => AMB,
            _ => continue,
        };
        if seen & bit != 0 {
            return Err(());
        }
        seen |= bit;
        match bit {
            UID => status.uids = parse_four(rest)?,
            GID => status.gids = parse_four(rest)?,
            GROUPS => status.groups = parse_u32s(rest)?,
            NNP => status.no_new_privs = parse_flag(rest)?,
            INH => status.cap_inh = parse_cap(rest)?,
            PRM => status.cap_prm = parse_cap(rest)?,
            EFF => status.cap_eff = parse_cap(rest)?,
            BND => status.cap_bnd = parse_cap(rest)?,
            AMB => status.cap_amb = parse_cap(rest)?,
            _ => return Err(()),
        }
    }
    if seen != NEED {
        return Err(());
    }
    Ok(status)
}

#[cfg(target_os = "linux")]
fn parse_u32s(text: &str) -> Result<Vec<u32>, ()> {
    let mut out = Vec::new();
    for part in text.split_whitespace() {
        out.push(part.parse().map_err(|_| ())?);
    }
    Ok(out)
}

#[cfg(target_os = "linux")]
fn parse_four(text: &str) -> Result<[u32; 4], ()> {
    let values = parse_u32s(text)?;
    values.try_into().map_err(|_| ())
}

#[cfg(target_os = "linux")]
fn parse_flag(text: &str) -> Result<u32, ()> {
    let value = text.parse::<u32>().map_err(|_| ())?;
    if value > 1 {
        return Err(());
    }
    Ok(value)
}

#[cfg(target_os = "linux")]
fn parse_cap(text: &str) -> Result<u128, ()> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(());
    }
    u128::from_str_radix(text, 16).map_err(|_| ())
}
