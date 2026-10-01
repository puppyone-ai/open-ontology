use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::error::{Result, err};

pub const STAGE_MARK: &str = "OO-DATAGEN-STAGE/v1\n";
const FORBIDDEN: &[&str] = &[
    "secret",
    "secrets",
    "private",
    "credentials",
    "customer-data",
    "customer",
    "customers",
    ".ssh",
    ".aws",
    ".gnupg",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exists {
    Fail,
    Check,
    Resume,
}

impl Exists {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "fail" => Some(Self::Fail),
            "check" => Some(Self::Check),
            "resume" => Some(Self::Resume),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct Placement {
    pub dest: PathBuf,
    pub stage: PathBuf,
    pub mark: PathBuf,
    pub parent: PathBuf,
    pub leaf: String,
    /// Caller path before canonicalization. `/tmp` and `/var` stay free of the
    /// Darwin `/private` prefix that `canonicalize` inserts.
    pub requested: PathBuf,
}

pub fn placement(scope: &Path, output: &Path) -> Result<Placement> {
    if output.as_os_str().is_empty() {
        return Err(err("output path is empty"));
    }
    reject_components(output)?;
    // Scope may be a symlink such as /tmp. The destination leaf is checked separately.
    let scope_meta = fs::metadata(scope).map_err(|_| err("scope does not exist"))?;
    if !scope_meta.is_dir() {
        return Err(err("scope is not a directory"));
    }
    let scope_canon = fs::canonicalize(scope)?;
    let joined = if output.is_absolute() {
        output.to_path_buf()
    } else {
        scope.join(output)
    };
    let parent = joined.parent().ok_or_else(|| err("output has no parent"))?;
    let leaf = joined
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| err("output name is not utf-8"))?
        .to_string();
    if leaf.is_empty() || leaf == "." || leaf == ".." || leaf.starts_with('.') {
        return Err(err("output name is not an explicit new directory"));
    }
    reject_name(&leaf)?;
    let parent_canon = fs::canonicalize(parent).map_err(|_| err("output parent does not exist"))?;
    if !parent_canon.starts_with(&scope_canon) {
        return Err(err("output escapes the explicit scope"));
    }
    if parent_canon == scope_canon && leaf.is_empty() {
        return Err(err("output escapes the explicit scope"));
    }
    let dest = parent_canon.join(&leaf);
    if dest == scope_canon {
        return Err(err("output is the scope itself"));
    }
    if let Ok(meta) = fs::symlink_metadata(&dest)
        && meta.file_type().is_symlink()
    {
        return Err(err("output path is a symlink"));
    }
    let stage = parent_canon.join(format!(".{leaf}.oo-datagen-stage"));
    let mark = parent_canon.join(format!(".{leaf}.oo-datagen-stage-mark"));
    Ok(Placement {
        dest,
        stage,
        mark,
        parent: parent_canon,
        leaf,
        requested: joined,
    })
}

fn reject_components(path: &Path) -> Result<()> {
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                let text = name.to_str().ok_or_else(|| err("path is not utf-8"))?;
                reject_name(text)?;
            }
            Component::CurDir => {}
            Component::ParentDir => return Err(err("output path contains '..'")),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    Ok(())
}

fn reject_name(name: &str) -> Result<()> {
    if name.chars().any(|ch| ch.is_ascii_control()) {
        return Err(err("path component contains an ascii control character"));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(err("path component contains a separator"));
    }
    let folded = name.to_ascii_lowercase();
    if FORBIDDEN.iter().any(|item| folded == *item) {
        return Err(err(format!(
            "refusing private or secret path component {name}"
        )));
    }
    Ok(())
}

/// Check the path the caller typed. Do not canonicalize first: `/tmp` on
/// Darwin is a symlink to `/private/tmp`, and `private` is a forbidden leaf.
pub fn reject_entry(path: &Path) -> Result<()> {
    for component in path.components() {
        if let Component::Normal(name) = component {
            let text = name.to_str().ok_or_else(|| err("path is not utf-8"))?;
            reject_name(text)?;
        }
    }
    Ok(())
}

pub fn validate_rel(rel: &str) -> Result<()> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains('\\') {
        return Err(err(format!("refusing dataset path {rel}")));
    }
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(err(format!("refusing dataset path {rel}")));
        }
        if part.chars().any(|ch| ch.is_ascii_control()) {
            return Err(err("path component contains an ascii control character"));
        }
    }
    Ok(())
}

pub fn remove_owned_stage(stage: &Path, mark: &Path) -> Result<()> {
    let text = fs::read_to_string(mark).map_err(|_| err("stage mark is missing or unreadable"))?;
    if text != STAGE_MARK {
        return Err(err("refusing to delete a stage without our mark"));
    }
    let name = stage
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| err("stage name is not utf-8"))?;
    if !name.starts_with('.') || !name.ends_with(".oo-datagen-stage") {
        return Err(err("refusing to delete a path that is not our stage"));
    }
    let meta = fs::symlink_metadata(stage)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(err("stage root is not a real directory"));
    }
    remove_tree_nofollow(stage)?;
    fs::remove_file(mark)?;
    Ok(())
}

fn remove_tree_nofollow(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || meta.is_file() {
        fs::remove_file(path)?;
        return Ok(());
    }
    if !meta.is_dir() {
        return Err(err("stage contains an unsupported file type"));
    }
    for entry in fs::read_dir(path)? {
        remove_tree_nofollow(&entry?.path())?;
    }
    fs::remove_dir(path)?;
    Ok(())
}

pub fn create_stage(stage: &Path, mark: &Path) -> Result<()> {
    if stage.exists() || mark.exists() || fs::symlink_metadata(stage).is_ok() {
        return Err(err("stage path already exists"));
    }
    fs::create_dir(stage)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(mark)?;
    file.write_all(STAGE_MARK.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

pub fn promote(stage: &Path, mark: &Path, dest: &Path) -> Result<()> {
    if fs::symlink_metadata(dest).is_ok() {
        return Err(err("destination appeared before promote"));
    }
    fs::rename(stage, dest)?;
    if fs::symlink_metadata(mark).is_ok() {
        fs::remove_file(mark)?;
    }
    Ok(())
}

pub fn create_new_file(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::{reject_entry, reject_name, validate_rel};

    #[test]
    fn rejects_customer_component_and_ascii_controls() {
        let customer = reject_name("customer").unwrap_err().to_string();
        assert!(customer.contains("component customer"), "{customer}");
        assert!(reject_name("customerx").is_ok());
        assert!(
            reject_name("customers")
                .unwrap_err()
                .to_string()
                .contains("customers")
        );
        let control = reject_name("bad\nname").unwrap_err().to_string();
        assert!(control.contains("ascii control"), "{control}");
        assert!(reject_name("ok").is_ok());
        assert!(
            validate_rel("a/\u{1}b")
                .unwrap_err()
                .to_string()
                .contains("ascii control")
        );
        assert!(validate_rel("a/\u{0}b").is_err());
        let entry = reject_entry(std::path::Path::new("bad\nname"))
            .unwrap_err()
            .to_string();
        assert!(entry.contains("ascii control"), "{entry}");
        assert!(
            reject_entry(std::path::Path::new("scope/customer"))
                .unwrap_err()
                .to_string()
                .contains("component customer")
        );
    }
}
