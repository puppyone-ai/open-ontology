use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::fault::Fault;
use crate::path;

pub fn ensure_available(dir: &Path, names: &[&str]) -> Result<(), Fault> {
    let dir = path::require_dir(dir)?;
    for name in names {
        path::require_absent(&dir.join(name))?;
        path::require_absent(&dir.join(format!("{name}.partial")))?;
    }
    Ok(())
}

pub fn publish(dir: &Path, files: &[(&str, &[u8])], cancel: Option<&Path>) -> Result<(), Fault> {
    let staged = stage(dir, files, true)?;
    commit(staged, cancel)
}

#[derive(Debug)]
struct Staged {
    partials: Vec<PathBuf>,
    finals: Vec<PathBuf>,
}

fn stage(dir: &Path, files: &[(&str, &[u8])], precheck: bool) -> Result<Staged, Fault> {
    let dir = path::require_dir(dir)?;
    let finals: Vec<PathBuf> = files.iter().map(|(name, _)| dir.join(name)).collect();
    let partials: Vec<PathBuf> = files
        .iter()
        .map(|(name, _)| dir.join(format!("{name}.partial")))
        .collect();
    if precheck {
        for path in finals.iter().chain(partials.iter()) {
            path::require_absent(path)?;
        }
    }
    let mut created = Vec::new();
    for (path, (_, bytes)) in partials.iter().zip(files) {
        let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(file) => file,
            Err(err) => {
                remove_files(&created);
                return Err(create_fault(err));
            }
        };
        created.push(path.clone());
        if let Err(fault) = write_synced(&mut file, bytes) {
            remove_files(&created);
            return Err(fault);
        }
    }
    Ok(Staged { partials, finals })
}

fn commit(staged: Staged, cancel: Option<&Path>) -> Result<(), Fault> {
    if let Some(path) = cancel {
        match path::cancel_requested(path) {
            Ok(false) => {}
            Ok(true) => {
                remove_files(&staged.partials);
                return Err(Fault::canceled());
            }
            Err(fault) => {
                remove_files(&staged.partials);
                return Err(fault);
            }
        }
    }
    let mut renamed = Vec::new();
    for (partial, final_path) in staged.partials.iter().zip(staged.finals.iter()) {
        if fs::rename(partial, final_path).is_err() {
            remove_files(&staged.partials);
            remove_files(&renamed);
            return Err(Fault::io());
        }
        renamed.push(final_path.clone());
    }
    Ok(())
}

fn write_synced(file: &mut File, bytes: &[u8]) -> Result<(), Fault> {
    file.write_all(bytes).map_err(|_| Fault::io())?;
    file.sync_all().map_err(|_| Fault::io())?;
    Ok(())
}

fn create_fault(err: io::Error) -> Fault {
    if err.kind() == io::ErrorKind::AlreadyExists || err.kind() == io::ErrorKind::IsADirectory {
        Fault::path()
    } else {
        Fault::io()
    }
}

fn remove_files(paths: &[PathBuf]) {
    for path in paths.iter().rev() {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::{commit, publish, stage};
    use crate::fault::Fault;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oo-a386-publish-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn files<'a>(csv: &'a [u8], svg: &'a [u8], manifest: &'a [u8]) -> [(&'a str, &'a [u8]); 3] {
        [
            ("analysis.csv", csv),
            ("analysis.svg", svg),
            ("analysis.manifest.json", manifest),
        ]
    }

    #[test]
    fn precheck_leaves_an_existing_final_and_creates_nothing() {
        let dir = scratch("precheck");
        fs::write(dir.join("analysis.csv"), b"keep").unwrap();
        let err = publish(&dir, &files(b"a", b"b", b"c"), None).unwrap_err();
        assert_eq!(err, Fault::path());
        assert_eq!(fs::read(dir.join("analysis.csv")).unwrap(), b"keep");
        assert!(!dir.join("analysis.csv.partial").exists());
        assert!(!dir.join("analysis.svg").exists());
        assert!(!dir.join("analysis.svg.partial").exists());
        assert!(!dir.join("analysis.manifest.json").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn blocked_later_create_removes_only_the_name_this_call_made() {
        let dir = scratch("residue");
        fs::create_dir(dir.join("analysis.svg.partial")).unwrap();
        let err = stage(&dir, &files(b"csv", b"svg", b"manifest"), false).unwrap_err();
        assert_eq!(err, Fault::path());
        assert!(!dir.join("analysis.csv.partial").exists());
        assert!(dir.join("analysis.svg.partial").is_dir());
        assert!(!dir.join("analysis.manifest.json.partial").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_after_partials_removes_them_before_rename() {
        let dir = scratch("cancel");
        let cancel = dir.join("stop");
        let staged = stage(&dir, &files(b"csv\r\n", b"<svg/>\n", b"{}\n"), true).unwrap();
        fs::write(&cancel, b"ignored").unwrap();
        let err = commit(staged, Some(cancel.as_path())).unwrap_err();
        assert_eq!(err, Fault::canceled());
        assert!(!dir.join("analysis.csv.partial").exists());
        assert!(!dir.join("analysis.svg.partial").exists());
        assert!(!dir.join("analysis.manifest.json.partial").exists());
        assert!(!dir.join("analysis.csv").exists());
        assert!(!dir.join("analysis.svg").exists());
        assert!(!dir.join("analysis.manifest.json").exists());
        assert_eq!(fs::read(&cancel).unwrap(), b"ignored");
        let _ = fs::remove_dir_all(&dir);
    }
}
