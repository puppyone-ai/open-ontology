use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use oo_control::HostLimits;

use crate::text::{self, sync_dir, write_atomic};
use crate::{Error, OWNER_UUID};

#[derive(Debug)]
pub struct Deployment {
    root: PathBuf,
    limits: HostLimits,
}

impl Deployment {
    pub fn create(root: &Path, limits: HostLimits) -> Result<Self, Error> {
        refuse_unowned(root)?;
        match fs::symlink_metadata(root) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(Error::Symlink),
            Ok(meta) if !meta.is_dir() => return Err(Error::NotOwnedRoot),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(root).map_err(|_| Error::Store)?;
            }
            Err(_) => return Err(Error::Store),
        }
        let canonical = fs::canonicalize(root).map_err(|_| Error::Store)?;
        if canonical.file_name().and_then(|name| name.to_str()) != Some(OWNER_UUID) {
            return Err(Error::NotOwnedRoot);
        }
        let owner = owner_text(&limits);
        let owner_path = root.join("owner.txt");
        match fs::symlink_metadata(&owner_path) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(Error::Symlink),
            Ok(meta) if !meta.is_file() => return Err(Error::Corrupt),
            Ok(_) => {
                let stored = fs::read_to_string(&owner_path).map_err(|_| Error::Corrupt)?;
                if stored != owner {
                    return Err(Error::Conflict);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                write_new(&owner_path, owner.as_bytes())?;
                sync_dir(root)?;
            }
            Err(_) => return Err(Error::Store),
        }
        for name in ["filehost", "inventory", "audit", "checkpoint", "control"] {
            let path = root.join(name);
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => return Err(Error::Symlink),
                Ok(meta) if !meta.is_dir() => return Err(Error::Corrupt),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&path).map_err(|_| Error::Store)?;
                }
                Err(_) => return Err(Error::Store),
            }
        }
        let inventory = root.join("inventory");
        if fs::symlink_metadata(inventory.join("registrations.txt")).is_err() {
            let body = b"oo.artifact-retention/v1\ncount 0\n".to_vec();
            text::write_hashed(&inventory, "registrations.txt", &body)?;
        }
        sync_dir(root)?;
        Ok(Self {
            root: canonical,
            limits,
        })
    }

    pub fn open(root: &Path) -> Result<Self, Error> {
        refuse_unowned(root)?;
        let meta = fs::symlink_metadata(root).map_err(|_| Error::NotOwnedRoot)?;
        if meta.file_type().is_symlink() {
            return Err(Error::Symlink);
        }
        if !meta.is_dir() {
            return Err(Error::NotOwnedRoot);
        }
        let canonical = fs::canonicalize(root).map_err(|_| Error::Store)?;
        if canonical.file_name().and_then(|name| name.to_str()) != Some(OWNER_UUID) {
            return Err(Error::NotOwnedRoot);
        }
        let stored = fs::read_to_string(canonical.join("owner.txt")).map_err(|_| Error::Corrupt)?;
        let limits = parse_owner(&stored)?;
        Ok(Self {
            root: canonical,
            limits,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn filehost_dir(&self) -> PathBuf {
        self.root.join("filehost")
    }

    pub fn limits(&self) -> HostLimits {
        self.limits
    }

    pub(crate) fn inventory_dir(&self) -> PathBuf {
        self.root.join("inventory")
    }

    pub(crate) fn audit_dir(&self) -> PathBuf {
        self.root.join("audit")
    }

    pub(crate) fn checkpoint_dir(&self) -> PathBuf {
        self.root.join("checkpoint")
    }

    pub fn control_dir(&self) -> PathBuf {
        self.root.join("control")
    }
}

fn refuse_unowned(root: &Path) -> Result<(), Error> {
    if root.as_os_str().is_empty() {
        return Err(Error::NotOwnedRoot);
    }
    if root.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    }) {
        return Err(Error::ForeignPath);
    }
    if root.file_name().and_then(|name| name.to_str()) != Some(OWNER_UUID) {
        return Err(Error::NotOwnedRoot);
    }
    Ok(())
}

fn owner_text(limits: &HostLimits) -> String {
    format!(
        "oo.artifact-retention/v1\nuuid {OWNER_UUID}\nmax_object_bytes {}\nmax_part_bytes {}\nmax_part_count {}\nmax_project_bytes {}\n",
        limits.max_object_bytes(),
        limits.max_part_bytes(),
        limits.max_part_count(),
        limits.max_project_bytes()
    )
}

fn parse_owner(text: &str) -> Result<HostLimits, Error> {
    let mut lines = text.lines();
    expect(&mut lines, "oo.artifact-retention/v1")?;
    expect(&mut lines, &format!("uuid {OWNER_UUID}"))?;
    let max_object_bytes = expect_u64(&mut lines, "max_object_bytes")?;
    let max_part_bytes = expect_u64(&mut lines, "max_part_bytes")?;
    let max_part_count =
        u32::try_from(expect_u64(&mut lines, "max_part_count")?).map_err(|_| Error::Corrupt)?;
    let max_project_bytes = expect_u64(&mut lines, "max_project_bytes")?;
    if lines.next().is_some() {
        return Err(Error::Corrupt);
    }
    HostLimits::try_new(
        max_object_bytes,
        max_part_bytes,
        max_part_count,
        max_project_bytes,
    )
    .map_err(|_| Error::Corrupt)
}

fn expect<'a>(lines: &mut impl Iterator<Item = &'a str>, expected: &str) -> Result<(), Error> {
    match lines.next() {
        Some(line) if line == expected => Ok(()),
        _ => Err(Error::Corrupt),
    }
}

fn expect_u64<'a>(lines: &mut impl Iterator<Item = &'a str>, key: &str) -> Result<u64, Error> {
    let Some(line) = lines.next() else {
        return Err(Error::Corrupt);
    };
    let Some(value) = line.strip_prefix(&format!("{key} ")) else {
        return Err(Error::Corrupt);
    };
    if value.is_empty() || (value.starts_with('0') && value != "0") {
        return Err(Error::Corrupt);
    }
    value.parse::<u64>().map_err(|_| Error::Corrupt)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| Error::Store)?;
    file.write_all(bytes).map_err(|_| Error::Store)?;
    file.sync_all().map_err(|_| Error::Store)?;
    Ok(())
}

pub(crate) fn write_checkpoint(
    deploy: &Deployment,
    phase: &str,
    registration_id: &str,
) -> Result<(), Error> {
    if !text::token_ok(phase) || !text::token_ok(registration_id) {
        return Err(Error::Corrupt);
    }
    let body = format!(
        "oo.artifact-retention/v1\nphase {phase}\nregistration_id {registration_id}\nabsence_verified false\ndeletion_success false\n"
    );
    write_atomic(&deploy.checkpoint_dir(), "reap.txt", body.as_bytes())
}

pub(crate) fn read_checkpoint(deploy: &Deployment) -> Result<Option<String>, Error> {
    let path = deploy.checkpoint_dir().join("reap.txt");
    match fs::read_to_string(&path) {
        Ok(text) => {
            let mut lines = text.lines();
            expect(&mut lines, "oo.artifact-retention/v1")?;
            let Some(phase_line) = lines.next() else {
                return Err(Error::Corrupt);
            };
            let Some(phase) = phase_line.strip_prefix("phase ") else {
                return Err(Error::Corrupt);
            };
            if !text::token_ok(phase) {
                return Err(Error::Corrupt);
            }
            let Some(id_line) = lines.next() else {
                return Err(Error::Corrupt);
            };
            if !id_line.starts_with("registration_id ") {
                return Err(Error::Corrupt);
            }
            expect(&mut lines, "absence_verified false")?;
            expect(&mut lines, "deletion_success false")?;
            if lines.next().is_some() {
                return Err(Error::Corrupt);
            }
            Ok(Some(phase.to_owned()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(Error::Corrupt),
    }
}
