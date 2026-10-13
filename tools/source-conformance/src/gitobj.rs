//! Read-only git object opens. This crate never writes the repository.

use std::path::{Component, Path};
use std::process::Command;

use crate::sha256_hex;

pub const MAX_OBJECT_BYTES: usize = 2_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub blob: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GitRef {
    pub commit: String,
    pub path: String,
    pub blob: String,
    pub sha256: String,
    pub byte_len: usize,
}

pub trait ObjectStore {
    fn has_commit(&self, commit: &str) -> bool;
    fn open(&self, commit: &str, path: &str) -> Result<Opened, String>;
}

pub struct GitStore<'a> {
    pub repo: &'a Path,
}

impl ObjectStore for GitStore<'_> {
    fn has_commit(&self, commit: &str) -> bool {
        if !valid_commit(commit) {
            return false;
        }
        git(self.repo, &["cat-file", "-t", commit]).is_ok_and(|bytes| trim_text(&bytes) == "commit")
    }

    fn open(&self, commit: &str, path: &str) -> Result<Opened, String> {
        if !valid_commit(commit) {
            return Err(format!("scope.commit_malformed: {commit}"));
        }
        if !valid_path(path) {
            return Err(format!("scope.path_rejected: {path}"));
        }
        let spec = format!("{commit}:{path}");
        let blob = trim_text(&git(self.repo, &["rev-parse", "--verify", &spec])?);
        if !valid_commit(&blob) {
            return Err(format!("scope.blob_malformed: {path}"));
        }
        let size_text = trim_text(&git(self.repo, &["cat-file", "-s", &blob])?);
        let size: usize = size_text.parse().map_err(|_| format!("scope.object_size: {path}"))?;
        if size > MAX_OBJECT_BYTES {
            return Err(format!("scope.object_too_large: {path}"));
        }
        let bytes = git(self.repo, &["cat-file", "blob", &blob])?;
        if bytes.len() != size {
            return Err(format!("scope.object_size: {path}"));
        }
        Ok(Opened { blob, bytes })
    }
}

#[derive(Default)]
pub struct MemoryStore {
    commits: std::collections::BTreeSet<String>,
    files: std::collections::BTreeMap<(String, String), Opened>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_commit(&mut self, commit: &str) {
        self.commits.insert(commit.to_owned());
    }

    pub fn insert(&mut self, commit: &str, path: &str, blob: &str, bytes: impl AsRef<[u8]>) {
        self.commits.insert(commit.to_owned());
        self.files.insert(
            (commit.to_owned(), path.to_owned()),
            Opened { blob: blob.to_owned(), bytes: bytes.as_ref().to_vec() },
        );
    }
}

impl ObjectStore for MemoryStore {
    fn has_commit(&self, commit: &str) -> bool {
        self.commits.contains(commit)
    }

    fn open(&self, commit: &str, path: &str) -> Result<Opened, String> {
        self.files
            .get(&(commit.to_owned(), path.to_owned()))
            .cloned()
            .ok_or_else(|| format!("scope.object_missing: {path}"))
    }
}

pub fn git_ref(commit: &str, path: &str, opened: &Opened) -> GitRef {
    GitRef {
        commit: commit.to_owned(),
        path: path.to_owned(),
        blob: opened.blob.clone(),
        sha256: sha256_hex(&opened.bytes),
        byte_len: opened.bytes.len(),
    }
}

pub fn valid_commit(text: &str) -> bool {
    text.len() == 40 && text.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn valid_path(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.contains('\0') || path.contains('\n') {
        return false;
    }
    Path::new(path).components().all(|component| matches!(component, Component::Normal(_)))
}

fn trim_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_owned()
}

fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-c")
        .arg("safe.directory=*")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .map_err(|error| format!("scope.git_failed: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim().lines().next().unwrap_or("git failed");
        return Err(format!("scope.object_missing: {detail}"));
    }
    Ok(output.stdout)
}
