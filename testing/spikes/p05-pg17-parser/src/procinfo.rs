//! `/proc` identity helpers. Stat fields only: pid, ppid, pgid, sid, starttime.
//! Command lines and environments are not read.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Desc {
    pub pid: u32,
    pub ppid: u32,
    pub pgid: u32,
    pub sid: u32,
    pub starttime: String,
}

pub fn proc_starttime_self() -> Option<String> {
    proc_starttime(std::process::id())
}

pub fn proc_starttime(pid: u32) -> Option<String> {
    read_desc(pid).map(|desc| desc.starttime)
}

pub fn read_desc(pid: u32) -> Option<Desc> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = text.rsplit_once(')')?.1;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // After `comm`, field 0 is state. ppid, pgrp, session follow. starttime is field 22 (index 19).
    let ppid = fields.get(1)?.parse().ok()?;
    let pgid = fields.get(2)?.parse().ok()?;
    let sid = fields.get(3)?.parse().ok()?;
    let starttime = fields.get(19)?.to_string();
    if starttime.is_empty() {
        return None;
    }
    Some(Desc { pid, ppid, pgid, sid, starttime })
}

/// `Some(true)` only when `pid` is a strict ppid-chain descendant of `ancestor`.
/// `None` means `/proc` could not be read. Callers fail closed on `None`.
pub fn is_descendant(pid: u32, ancestor: u32) -> Option<bool> {
    if pid == 0 || ancestor == 0 || pid == ancestor {
        return Some(false);
    }
    let mut current = pid;
    for _ in 0..64 {
        let desc = read_desc(current)?;
        if desc.ppid == ancestor {
            return Some(true);
        }
        if desc.ppid <= 1 || desc.ppid == current {
            return Some(false);
        }
        current = desc.ppid;
    }
    Some(false)
}

/// Direct and indirect children of `root`, by ppid ancestry. `root` itself is omitted.
/// A scan error is not an empty tree.
pub fn descendants(root: u32) -> io::Result<Vec<Desc>> {
    let mut by_pid: HashMap<u32, Desc> = HashMap::new();
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry.file_name().to_str().and_then(|text| text.parse::<u32>().ok()) else {
            continue;
        };
        let Some(desc) = read_desc(pid) else {
            continue;
        };
        children.entry(desc.ppid).or_default().push(pid);
        by_pid.insert(pid, desc);
    }
    let mut out = Vec::new();
    let mut stack = children.get(&root).cloned().unwrap_or_default();
    let mut seen = HashSet::new();
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if seen.len() > 4096 {
            return Err(io::Error::other("descendant scan cap"));
        }
        let Some(desc) = by_pid.get(&pid) else {
            continue;
        };
        out.push(desc.clone());
        if let Some(kids) = children.get(&pid) {
            stack.extend(kids);
        }
    }
    Ok(out)
}

/// Same starttime means the pid was not reused and the process is still that task.
pub fn same_start(pid: u32, starttime: &str) -> bool {
    matches!(proc_starttime(pid), Some(current) if current == starttime)
}
