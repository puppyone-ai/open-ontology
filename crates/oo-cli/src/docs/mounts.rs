//! Physical mount of a pack (10 §7.7): write `<dir>/.context` atomically.
//!
//! The files go to `<dir>/.context.tmp-<sid>` (0700), then become 0444 files
//! in 0555 directories, are fsynced, and the directory is renamed to
//! `.context`. Any failure removes the temp directory, so no half-written
//! mount is ever visible. An existing `.context` is never replaced or
//! deleted: not ours to judge (L2 has no share lifecycle to ask).

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

/// The mount directory name inside the target.
pub const MOUNT_DIR: &str = ".context";

/// Why a mount did not happen.
#[derive(Debug, PartialEq, Eq)]
pub enum MountError {
    /// `<dir>/.context` exists (`DOCS_SHARE_TARGET_OCCUPIED`).
    Occupied,
    /// The target is not an acceptable directory (`DOCS_SHARE_TARGET_INVALID`).
    Invalid(&'static str),
    /// A file path of the pack is not a plain relative path.
    BadPath,
    /// Writing failed (`DOCS_SHARE_TARGET_WRITE_FAILED`).
    Write(String),
}

/// Check `--to` (10 §7.7 point 1): an existing absolute directory, resolved
/// through symlinks, that is not `/`, not the home directory and not inside
/// the oo state directory. Returns the canonical path.
pub fn check_target(
    to: &str,
    home: Option<&Path>,
    state: Option<&Path>,
) -> Result<PathBuf, MountError> {
    let p = Path::new(to);
    if !p.is_absolute() {
        return Err(MountError::Invalid("--to must be an absolute path"));
    }
    let real = fs::canonicalize(p).map_err(|_| MountError::Invalid("--to does not exist"))?;
    if !fs::metadata(&real).is_ok_and(|m| m.is_dir()) {
        return Err(MountError::Invalid("--to is not a directory"));
    }
    if real == Path::new("/") {
        return Err(MountError::Invalid("--to must not be the filesystem root"));
    }
    if let Some(h) = home.and_then(|h| fs::canonicalize(h).ok())
        && real == h
    {
        return Err(MountError::Invalid(
            "--to must not be the home directory itself",
        ));
    }
    if let Some(s) = state.and_then(|s| fs::canonicalize(s).ok())
        && real.starts_with(&s)
    {
        return Err(MountError::Invalid(
            "--to must not be inside the oo state directory",
        ));
    }
    Ok(real)
}

/// `rel` is a relative path of normal components only.
pub fn safe_relative(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.contains(['\\', '\0'])
        && rel
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

fn io<E: std::fmt::Display>(e: E) -> MountError {
    MountError::Write(e.to_string())
}

/// Make a tree writable again and delete it (it may be 0555 inside).
pub fn remove_tree(dir: &Path) {
    fn open_up(p: &Path) {
        let Ok(m) = fs::symlink_metadata(p) else {
            return;
        };
        if m.file_type().is_symlink() {
            return;
        }
        if m.is_dir() {
            let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o700));
            if let Ok(rd) = fs::read_dir(p) {
                for e in rd.flatten() {
                    open_up(&e.path());
                }
            }
        }
    }
    open_up(dir);
    let _ = fs::remove_dir_all(dir);
}

fn fsync_dir(p: &Path) -> Result<(), MountError> {
    File::open(p).and_then(|f| f.sync_all()).map_err(io)
}

fn write_tree(tmp: &Path, files: &[(String, Vec<u8>)]) -> Result<(), MountError> {
    DirBuilder::new().mode(0o700).create(tmp).map_err(io)?;
    for (rel, bytes) in files {
        let path = tmp.join(rel);
        if let Some(parent) = path.parent() {
            DirBuilder::new()
                .mode(0o700)
                .recursive(true)
                .create(parent)
                .map_err(io)?;
        }
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(io)?;
        f.write_all(bytes).map_err(io)?;
        f.sync_all().map_err(io)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).map_err(io)?;
    }
    // Directories last, deepest first: 0555 would block writing into them.
    let mut dirs: Vec<PathBuf> = Vec::new();
    for (rel, _) in files {
        let mut p = tmp.join(rel);
        while p.pop() && p.starts_with(tmp) {
            if !dirs.contains(&p) {
                dirs.push(p.clone());
            }
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in &dirs {
        fsync_dir(d)?;
        fs::set_permissions(d, fs::Permissions::from_mode(0o555)).map_err(io)?;
    }
    Ok(())
}

/// Install `files` (relative path, bytes) as `<target>/.context`; returns the
/// final path. `tag` makes the temp name unique (the share id).
pub fn install(
    target: &Path,
    tag: &str,
    files: &[(String, Vec<u8>)],
) -> Result<PathBuf, MountError> {
    if files.iter().any(|(p, _)| !safe_relative(p)) {
        return Err(MountError::BadPath);
    }
    let final_path = target.join(MOUNT_DIR);
    if fs::symlink_metadata(&final_path).is_ok() {
        return Err(MountError::Occupied);
    }
    let tmp = target.join(format!("{MOUNT_DIR}.tmp-{tag}"));
    remove_tree(&tmp);
    let outcome = (|| {
        write_tree(&tmp, files)?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o555)).map_err(io)?;
        fsync_dir(target)?;
        // A mount that appeared meanwhile still wins: rename onto an existing
        // directory fails for a non-empty one, but check explicitly first.
        if fs::symlink_metadata(&final_path).is_ok() {
            return Err(MountError::Occupied);
        }
        fs::rename(&tmp, &final_path).map_err(io)?;
        fsync_dir(target)
    })();
    match outcome {
        Ok(()) => Ok(final_path),
        Err(e) => {
            remove_tree(&tmp);
            Err(e)
        }
    }
}

/// Mode bits of a path, for tests.
#[cfg(test)]
pub fn mode_of(p: &Path) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(p).ok().map(|m| m.mode() & 0o777)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("oo-mounts-{name}-{}", std::process::id()));
        remove_tree(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn files() -> Vec<(String, Vec<u8>)> {
        vec![
            ("BRIEF.md".into(), b"brief\n".to_vec()),
            ("people/a-1234abcd.md".into(), "林川\n".as_bytes().to_vec()),
            ("refs/0001.md".into(), b"ref\n".to_vec()),
        ]
    }

    #[test]
    fn a_mount_is_complete_read_only_and_leaves_no_temp_directory() {
        let b = base("ok");
        let at = install(&b, "s1", &files()).unwrap();
        assert_eq!(at, b.join(".context"));
        assert_eq!(fs::read(at.join("BRIEF.md")).unwrap(), b"brief\n");
        assert_eq!(
            fs::read(at.join("people/a-1234abcd.md")).unwrap(),
            "林川\n".as_bytes()
        );
        assert_eq!(mode_of(&at.join("BRIEF.md")), Some(0o444));
        assert_eq!(mode_of(&at.join("people")), Some(0o555));
        assert_eq!(mode_of(&at), Some(0o555));
        // Read only: nothing can be created inside.
        assert!(
            File::create(at.join("new.md")).is_err()
                || std::env::var("USER").as_deref() == Ok("root")
        );
        let names: Vec<_> = fs::read_dir(&b)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names, [".context"]);
        remove_tree(&b);
    }

    #[test]
    fn an_occupied_target_is_refused_and_left_alone() {
        let b = base("occupied");
        fs::create_dir_all(b.join(".context")).unwrap();
        fs::write(b.join(".context/mine.md"), "keep").unwrap();
        assert_eq!(install(&b, "s1", &files()), Err(MountError::Occupied));
        assert_eq!(
            fs::read_to_string(b.join(".context/mine.md")).unwrap(),
            "keep"
        );
        // A symlink named .context counts too, and is not followed.
        let c = base("occupied-link");
        std::os::unix::fs::symlink("/tmp", c.join(".context")).unwrap();
        assert_eq!(install(&c, "s1", &files()), Err(MountError::Occupied));
        remove_tree(&b);
        remove_tree(&c);
    }

    #[test]
    fn a_failure_leaves_neither_a_mount_nor_a_temp_directory() {
        let b = base("fail");
        // Two entries that collide (a file and a directory of the same name).
        let bad = vec![
            ("a".to_owned(), b"x".to_vec()),
            ("a/b.md".to_owned(), b"y".to_vec()),
        ];
        assert!(matches!(install(&b, "s1", &bad), Err(MountError::Write(_))));
        let left: Vec<_> = fs::read_dir(&b).unwrap().flatten().collect();
        assert!(left.is_empty(), "{left:?}");
        remove_tree(&b);
    }

    #[test]
    fn hostile_paths_are_refused_before_anything_is_written() {
        let b = base("paths");
        for p in [
            "../x", "/abs", "a//b", "a/../b", "", "a\\b", "a/", ".", "x\0y",
        ] {
            let r = install(&b, "s", &[(p.to_owned(), b"z".to_vec())]);
            assert_eq!(r, Err(MountError::BadPath), "{p:?}");
        }
        assert_eq!(fs::read_dir(&b).unwrap().count(), 0);
        remove_tree(&b);
    }

    #[test]
    fn targets_are_checked() {
        let b = base("target");
        let ok = check_target(b.to_str().unwrap(), None, None).unwrap();
        assert!(ok.is_absolute());
        assert!(check_target("relative", None, None).is_err());
        assert!(check_target("/nonexistent-oo-dir", None, None).is_err());
        assert!(check_target("/", None, None).is_err());
        assert!(check_target(b.to_str().unwrap(), Some(&b), None).is_err());
        let inside = b.join("state").join("x");
        fs::create_dir_all(&inside).unwrap();
        assert!(check_target(inside.to_str().unwrap(), None, Some(&b.join("state"))).is_err());
        let file = b.join("f");
        fs::write(&file, "x").unwrap();
        assert!(check_target(file.to_str().unwrap(), None, None).is_err());
        remove_tree(&b);
    }
}
