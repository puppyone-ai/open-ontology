//! UUID-owned fixture paths. A foreign path is refused. A missing path is not absence.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path};

pub const COLLECT_BOUND: u64 = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Owned,
    Missing,
    Foreign,
}

pub fn classify(root: &Path, path: &Path, uuid: &str) -> Class {
    if uuid.is_empty() || !root.is_absolute() || !path.is_absolute() {
        return Class::Foreign;
    }
    if !has_component(root, uuid) || !has_component(path, uuid) || !lexical_under(root, path) {
        return Class::Foreign;
    }
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Class::Missing,
        Err(_) => Class::Foreign,
        Ok(meta) if meta.file_type().is_symlink() => Class::Foreign,
        Ok(_) => match (fs::canonicalize(root), fs::canonicalize(path)) {
            (Ok(root_canon), Ok(path_canon))
                if path_canon.starts_with(&root_canon) && has_component(&path_canon, uuid) =>
            {
                Class::Owned
            }
            _ => Class::Foreign,
        },
    }
}

pub fn overlaps(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

pub fn artifact_class(workspace: &Path, artifact: &Path, uuid: &str) -> Class {
    if !artifact.is_absolute() || !lexical_under(workspace, artifact) || !has_component(artifact, uuid) {
        return Class::Foreign;
    }
    match fs::symlink_metadata(artifact) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Class::Missing,
        Err(_) => Class::Foreign,
        Ok(meta) if meta.file_type().is_symlink() => Class::Foreign,
        Ok(meta) if meta.is_file() => Class::Owned,
        Ok(_) => Class::Foreign,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReadBound {
    Ready(Vec<u8>),
    TooBig,
    Missing,
    Foreign,
}

pub fn read_bounded(workspace: &Path, artifact: &Path, uuid: &str) -> ReadBound {
    match artifact_class(workspace, artifact, uuid) {
        Class::Foreign => return ReadBound::Foreign,
        Class::Missing => return ReadBound::Missing,
        Class::Owned => {}
    }
    let file = match File::open(artifact) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return ReadBound::Missing,
        Err(_) => return ReadBound::Foreign,
    };
    let mut buf = Vec::new();
    if file.take(COLLECT_BOUND.saturating_add(1)).read_to_end(&mut buf).is_err() {
        return ReadBound::Foreign;
    }
    if u64::try_from(buf.len()).unwrap_or(u64::MAX) > COLLECT_BOUND {
        return ReadBound::TooBig;
    }
    ReadBound::Ready(buf)
}

pub fn destroy_owned(root: &Path, path: &Path, uuid: &str) -> Result<(), &'static str> {
    if classify(root, path, uuid) != Class::Owned {
        return Err("not_owned");
    }
    if contains_symlink(path)? {
        return Err("symlink_refused");
    }
    remove_tree(path)?;
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err("still_present"),
    }
}

fn contains_symlink(path: &Path) -> Result<bool, &'static str> {
    let meta = fs::symlink_metadata(path).map_err(|_| "metadata")?;
    if meta.file_type().is_symlink() {
        return Ok(true);
    }
    if !meta.is_dir() {
        return Ok(false);
    }
    let entries = fs::read_dir(path).map_err(|_| "read_dir")?;
    for entry in entries {
        let entry = entry.map_err(|_| "read_dir")?;
        if contains_symlink(&entry.path())? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn remove_tree(path: &Path) -> Result<(), &'static str> {
    let meta = fs::symlink_metadata(path).map_err(|_| "metadata")?;
    if meta.is_dir() {
        let entries = fs::read_dir(path).map_err(|_| "read_dir")?;
        for entry in entries {
            let entry = entry.map_err(|_| "read_dir")?;
            remove_tree(&entry.path())?;
        }
        fs::remove_dir(path).map_err(|_| "remove")?;
    } else {
        fs::remove_file(path).map_err(|_| "remove")?;
    }
    Ok(())
}

fn has_component(path: &Path, uuid: &str) -> bool {
    path.components().any(|component| match component {
        Component::Normal(value) => value == std::ffi::OsStr::new(uuid),
        _ => false,
    })
}

fn lexical_under(root: &Path, path: &Path) -> bool {
    match (normalized(root), normalized(path)) {
        (Some(root_parts), Some(path_parts)) => path_parts.starts_with(&root_parts),
        _ => false,
    }
}

fn normalized(path: &Path) -> Option<Vec<Component<'_>>> {
    let mut out = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => return None,
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{classify, destroy_owned, read_bounded, Class, ReadBound, COLLECT_BOUND};
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch() -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("oo-a295-path-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn missing_and_foreign_are_distinct_and_destroy_verifies_absence() {
        let base = scratch();
        let uuid = "eef831aa-dba4-4af2-a78b-541bc9e0a295";
        let root = base.join(uuid);
        let owned = root.join("workspace");
        fs::create_dir_all(&owned).unwrap();
        fs::write(owned.join("note.txt"), b"note").unwrap();
        let missing = root.join("absent");
        let foreign = base.join("foreign");
        fs::create_dir_all(&foreign).unwrap();
        fs::write(foreign.join("keep.txt"), b"keep").unwrap();

        assert_eq!(classify(&root, &owned, uuid), Class::Owned);
        assert_eq!(classify(&root, &missing, uuid), Class::Missing);
        assert_eq!(classify(&root, &foreign, uuid), Class::Foreign);
        assert_eq!(classify(&root, &base.join(uuid).join("..").join("foreign"), uuid), Class::Foreign);

        let huge = owned.join("huge.bin");
        {
            let mut file = fs::File::create(&huge).unwrap();
            file.write_all(&vec![1u8; usize::try_from(COLLECT_BOUND).unwrap() + 8]).unwrap();
        }
        assert_eq!(read_bounded(&owned, &huge, uuid), ReadBound::TooBig);
        let small = owned.join("small.txt");
        fs::write(&small, b"ok").unwrap();
        assert_eq!(read_bounded(&owned, &small, uuid), ReadBound::Ready(b"ok".to_vec()));
        destroy_owned(&root, &owned, uuid).unwrap();
        assert!(fs::symlink_metadata(&owned).is_err());
        assert_eq!(fs::read(foreign.join("keep.txt")).unwrap(), b"keep");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn symlink_inside_an_owned_tree_is_not_destroyed() {
        let base = scratch();
        let uuid = "eef831aa-dba4-4af2-a78b-541bc9e0a295";
        let root = base.join(uuid);
        let owned = root.join("workspace");
        fs::create_dir_all(&owned).unwrap();
        fs::write(owned.join("note.txt"), b"note").unwrap();
        std::os::unix::fs::symlink("note.txt", owned.join("link")).unwrap();
        assert!(destroy_owned(&root, &owned, uuid).is_err());
        assert_eq!(fs::read(owned.join("note.txt")).unwrap(), b"note");
        let _ = fs::remove_dir_all(&base);
    }
}
