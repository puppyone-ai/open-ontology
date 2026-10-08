//! Execution journal and owned-file cleanup.
//!
//! `inflight` means a producer call started and the response was not recorded.
//! The next run does not send that call again. Cleanup removes only regular
//! files named in `owned.txt` under the state directory.

use std::fs;
use std::path::{Path, PathBuf};

use crate::assess::CONTRACT;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeLine {
    pub id: String,
    pub status: String,
    pub operation: String,
    pub request_sha256: String,
    pub response_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Journal {
    pub source_head: String,
    pub edges: Vec<EdgeLine>,
}

pub fn load(state: &Path) -> Result<Option<Journal>, &'static str> {
    let path = state.join("journal.txt");
    if !path.exists() {
        return Ok(None);
    }
    let text = read_file(&path)?;
    let mut lines = text.lines();
    let header = lines.next().unwrap_or("");
    if header != format!("contract {CONTRACT}") {
        return Err("journal");
    }
    let source = lines.next().unwrap_or("");
    let Some(source_head) = source.strip_prefix("source_head ") else {
        return Err("journal");
    };
    let mut edges = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split(' ');
        if parts.next() != Some("edge") {
            return Err("journal");
        }
        let id = parts.next().ok_or("journal")?.to_owned();
        let status = parts.next().ok_or("journal")?.to_owned();
        let operation = parts.next().ok_or("journal")?.to_owned();
        let request_sha256 = dash(parts.next().ok_or("journal")?);
        let response_sha256 = dash(parts.next().ok_or("journal")?);
        if parts.next().is_some() {
            return Err("journal");
        }
        edges.push(EdgeLine { id, status, operation, request_sha256, response_sha256 });
    }
    Ok(Some(Journal { source_head: source_head.to_owned(), edges }))
}

pub fn store(state: &Path, source_head: &str, edges: &[EdgeLine]) -> Result<(), &'static str> {
    ensure_dir(state)?;
    let mut text = format!("contract {CONTRACT}\nsource_head {source_head}\n");
    for edge in edges {
        text.push_str("edge ");
        text.push_str(&edge.id);
        text.push(' ');
        text.push_str(&edge.status);
        text.push(' ');
        text.push_str(if edge.operation.is_empty() { "-" } else { &edge.operation });
        text.push(' ');
        text.push_str(&shown(&edge.request_sha256));
        text.push(' ');
        text.push_str(&shown(&edge.response_sha256));
        text.push('\n');
    }
    write_new(&state.join("journal.txt"), text.as_bytes())
}

pub fn inflight(state: &Path) -> Result<Option<String>, &'static str> {
    let path = state.join("inflight");
    if !path.exists() {
        return Ok(None);
    }
    let text = read_file(&path)?;
    let id = text.trim();
    if !id.starts_with("OC-") || id.contains(' ') {
        return Err("inflight");
    }
    Ok(Some(id.to_owned()))
}

pub fn mark_inflight(state: &Path, id: &str) -> Result<(), &'static str> {
    ensure_dir(state)?;
    write_new(&state.join("inflight"), id.as_bytes())
}

pub fn clear_inflight(state: &Path) -> Result<(), &'static str> {
    let path = state.join("inflight");
    let meta = fs::symlink_metadata(&path).map_err(|_| "inflight")?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("inflight");
    }
    fs::remove_file(path).map_err(|_| "inflight")
}

pub fn cleanup(state: &Path) -> Result<(), &'static str> {
    ensure_dir(state)?;
    let owned = state.join("owned.txt");
    let text = if owned.exists() { read_file(&owned)? } else { String::new() };
    for line in text.lines() {
        if line.is_empty() || line.contains("..") || line.starts_with('/') || line.contains('\\') {
            return Err("owned");
        }
        let path = state.join(line);
        let meta = fs::symlink_metadata(&path).map_err(|_| "owned")?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err("owned");
        }
        let relative = path.strip_prefix(state).map_err(|_| "owned")?;
        if relative.components().count() == 0 {
            return Err("owned");
        }
        fs::remove_file(&path).map_err(|_| "owned")?;
    }
    write_new(&owned, b"")
}

fn shown(value: &str) -> String {
    if value.is_empty() { "-".to_owned() } else { value.to_owned() }
}

fn dash(value: &str) -> String {
    if value == "-" { String::new() } else { value.to_owned() }
}

fn ensure_dir(state: &Path) -> Result<(), &'static str> {
    let meta = fs::symlink_metadata(state).map_err(|_| "state")?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("state");
    }
    Ok(())
}

fn read_file(path: &Path) -> Result<String, &'static str> {
    let meta = fs::symlink_metadata(path).map_err(|_| "state")?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > 1_000_000 {
        return Err("state");
    }
    let bytes = fs::read(path).map_err(|_| "state")?;
    String::from_utf8(bytes).map_err(|_| "state")
}

fn write_new(path: &PathBuf, bytes: &[u8]) -> Result<(), &'static str> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        return Err("state");
    }
    fs::write(path, bytes).map_err(|_| "state")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{cleanup, inflight, load, mark_inflight, store, EdgeLine};

    #[test]
    fn cleanup_removes_only_listed_files_and_keeps_the_journal() {
        let state = std::env::temp_dir().join(format!("oo-a403-state-{}", std::process::id()));
        let _ = fs::remove_dir_all(&state);
        fs::create_dir(&state).expect("dir");
        let edges = [EdgeLine {
            id: "OC-01".to_owned(),
            status: "blocked_contract".to_owned(),
            operation: "-".to_owned(),
            request_sha256: String::new(),
            response_sha256: String::new(),
        }];
        store(&state, "abc", &edges).expect("store");
        fs::write(state.join("tmp-artifact"), b"partial").expect("artifact");
        fs::write(state.join("owned.txt"), b"tmp-artifact\n").expect("owned");
        cleanup(&state).expect("cleanup");
        assert!(!state.join("tmp-artifact").exists());
        assert!(state.join("journal.txt").exists());
        let loaded = load(&state).expect("load").expect("journal");
        assert_eq!(loaded.edges[0].status, "blocked_contract");
        assert!(loaded.edges[0].request_sha256.is_empty());
        mark_inflight(&state, "OC-01").expect("mark");
        assert_eq!(inflight(&state).expect("read").as_deref(), Some("OC-01"));
        let _ = fs::remove_dir_all(&state);
    }
}
