//! Identity-checked cleanup for a child this process spawned.
//!
//! `kill` and `ps` are validated absolute programs. The caller `PATH` is never
//! consulted. A same-group signal does not contain a descendant that called
//! `setsid`. Those descendants are recorded by pid and start token and signaled
//! individually. A survivor, or a tree that cannot be observed, is cleanup
//! incomplete: the caller must fail and must not reuse the capture.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use std::os::unix::fs::MetadataExt;

pub struct Cleanup {
    pub incomplete: bool,
    pub survivors: Vec<u32>,
}

struct Proc {
    pid: u32,
    ppid: u32,
    pgrp: u32,
    start: String,
}

pub struct OwnedTree {
    root: u32,
    root_start: String,
    root_pgrp: u32,
    members: BTreeMap<u32, String>,
    kill: Result<PathBuf, String>,
    list_failed: bool,
    ever_listed: bool,
}

impl OwnedTree {
    pub fn capture(root: u32) -> Self {
        let mut tree = Self {
            root,
            root_start: String::new(),
            root_pgrp: root,
            members: BTreeMap::new(),
            kill: trusted_tool("kill"),
            list_failed: false,
            ever_listed: false,
        };
        if root > 1 {
            tree.refresh();
        }
        tree
    }

    pub fn refresh(&mut self) {
        match list_processes() {
            Ok(procs) => {
                self.list_failed = false;
                self.ever_listed = true;
                self.absorb(&procs);
            }
            Err(_) => self.list_failed = true,
        }
    }

    /// Signal every recorded member whose start token still matches.
    /// The direct child may remain a zombie until the caller waits on it.
    pub fn kill_recorded(&mut self) -> Result<(), String> {
        let kill = match &self.kill {
            Ok(path) => path.clone(),
            Err(error) => return Err(error.clone()),
        };
        self.refresh();
        let procs = match list_processes() {
            Ok(procs) => procs,
            Err(error) => {
                self.list_failed = true;
                return Err(error);
            }
        };
        for pid in leaf_order(&self.members, &procs) {
            if pid <= 1 || pid == std::process::id() {
                continue;
            }
            let Some(start) = self.members.get(&pid).cloned() else {
                continue;
            };
            let Some(proc) = procs.iter().find(|proc| proc.pid == pid) else {
                continue;
            };
            if !start.is_empty() && proc.start != start {
                continue;
            }
            signal_target(&kill, &pid.to_string())?;
        }
        self.signal_group(&kill, &procs)
    }

    /// Caller waits the direct child before this, so that zombie is not a survivor.
    pub fn confirm(&mut self, budget: Duration) -> Cleanup {
        let started = Instant::now();
        loop {
            self.refresh();
            let alive = self.live_pids();
            if self.list_failed {
                let incomplete = !self.members.is_empty() || !self.ever_listed;
                return Cleanup {
                    incomplete,
                    survivors: alive,
                };
            }
            if alive.is_empty() {
                return Cleanup {
                    incomplete: false,
                    survivors: Vec::new(),
                };
            }
            if started.elapsed() >= budget || self.kill.is_err() {
                return Cleanup {
                    incomplete: true,
                    survivors: alive,
                };
            }
            if self.kill_recorded().is_err() {
                return Cleanup {
                    incomplete: true,
                    survivors: alive,
                };
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    }

    fn absorb(&mut self, procs: &[Proc]) {
        if let Some(root) = procs.iter().find(|proc| proc.pid == self.root) {
            if self.root_start.is_empty() {
                self.root_start.clone_from(&root.start);
                self.root_pgrp = root.pgrp;
                self.members.insert(self.root, root.start.clone());
            } else if root.start == self.root_start {
                self.root_pgrp = root.pgrp;
            }
        }
        let self_pid = std::process::id();
        let mut grew = true;
        while grew {
            grew = false;
            for proc in procs {
                if proc.pid <= 1 || proc.pid == self_pid || self.members.contains_key(&proc.pid) {
                    continue;
                }
                let from_member = self.members.contains_key(&proc.ppid);
                let root_live = procs.iter().any(|item| {
                    item.pid == self.root
                        && !self.root_start.is_empty()
                        && item.start == self.root_start
                });
                let in_created_group = root_live
                    && self.root > 1
                    && self.root_pgrp == self.root
                    && proc.pgrp == self.root;
                if from_member || in_created_group {
                    self.members.insert(proc.pid, proc.start.clone());
                    grew = true;
                }
            }
        }
    }

    fn live_pids(&mut self) -> Vec<u32> {
        let procs = match list_processes() {
            Ok(procs) => {
                self.list_failed = false;
                self.ever_listed = true;
                procs
            }
            Err(_) => {
                self.list_failed = true;
                return self.members.keys().copied().collect();
            }
        };
        self.members.retain(
            |pid, start| match procs.iter().find(|proc| proc.pid == *pid) {
                None => false,
                Some(proc) if !start.is_empty() && proc.start != *start => false,
                Some(_) => true,
            },
        );
        self.members.keys().copied().collect()
    }

    fn signal_group(&self, kill: &Path, procs: &[Proc]) -> Result<(), String> {
        if self.root <= 1 || self.root_start.is_empty() || self.root_pgrp != self.root {
            return Ok(());
        }
        let same = procs.iter().any(|proc| {
            proc.pid == self.root && proc.pgrp == self.root && proc.start == self.root_start
        });
        if !same {
            return Ok(());
        }
        signal_target(kill, &format!("-{}", self.root))
    }
}

pub fn trusted_tool(name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(format!("tool name {name} is not a single path component"));
    }
    let mut errors = Vec::new();
    for root in ["/bin", "/usr/bin"] {
        match validate_tool(&Path::new(root).join(name), name) {
            Ok(path) => return Ok(path),
            Err(error) => errors.push(error),
        }
    }
    Err(errors.join("; "))
}

fn validate_tool(candidate: &Path, name: &str) -> Result<PathBuf, String> {
    let canon = candidate
        .canonicalize()
        .map_err(|error| format!("{}: {error}", candidate.display()))?;
    let parent = canon.parent().and_then(|path| path.to_str()).unwrap_or("");
    let file = canon
        .file_name()
        .and_then(|file| file.to_str())
        .unwrap_or("");
    if file != name || (parent != "/bin" && parent != "/usr/bin") {
        return Err(format!(
            "{} canonicalizes outside /bin/{name} and /usr/bin/{name}",
            candidate.display()
        ));
    }
    let metadata =
        fs::metadata(&canon).map_err(|error| format!("metadata {}: {error}", canon.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} is not a regular file", canon.display()));
    }
    if metadata.uid() != 0 {
        return Err(format!("{} is not owned by uid 0", canon.display()));
    }
    let mode = metadata.mode();
    if mode & 0o022 != 0 {
        return Err(format!("{} is group or world writable", canon.display()));
    }
    if mode & 0o100 == 0 {
        return Err(format!("{} is not owner-executable", canon.display()));
    }
    Ok(canon)
}

fn list_processes() -> Result<Vec<Proc>, String> {
    if Path::new("/proc/1/stat").is_file() {
        list_linux()
    } else {
        list_via_ps()
    }
}

fn list_linux() -> Result<Vec<Proc>, String> {
    let entries = fs::read_dir("/proc").map_err(|error| format!("read /proc: {error}"))?;
    let mut procs = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(text) = name.to_str() else {
            continue;
        };
        if !text.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Ok(pid) = text.parse::<u32>() else {
            continue;
        };
        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        if let Some(proc) = parse_linux_stat(pid, &stat) {
            procs.push(proc);
        }
    }
    if procs.is_empty() {
        return Err("proc listed no processes".to_owned());
    }
    Ok(procs)
}

fn parse_linux_stat(pid: u32, stat: &str) -> Option<Proc> {
    let close = stat.rfind(')')?;
    let rest: Vec<&str> = stat[close + 1..].split_whitespace().collect();
    if rest.len() < 20 {
        return None;
    }
    let ppid = rest[1].parse().ok()?;
    let pgrp = rest[2].parse().ok()?;
    let start = rest[19].to_owned();
    if start.is_empty() {
        return None;
    }
    Some(Proc {
        pid,
        ppid,
        pgrp,
        start,
    })
}

fn list_via_ps() -> Result<Vec<Proc>, String> {
    let ps = trusted_tool("ps")?;
    let output = Command::new(&ps)
        .args(["-ax", "-o", "pid=,ppid=,pgid=,state=,lstart="])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("ps: {error}"))?;
    if !output.status.success() {
        return Err(format!("ps exited {}", output.status.code().unwrap_or(-1)));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut procs = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(pid) = parts.next().and_then(|item| item.parse::<u32>().ok()) else {
            continue;
        };
        let Some(ppid) = parts.next().and_then(|item| item.parse::<u32>().ok()) else {
            continue;
        };
        let Some(pgrp) = parts.next().and_then(|item| item.parse::<u32>().ok()) else {
            continue;
        };
        if parts.next().is_none() {
            continue;
        }
        let start = parts.collect::<Vec<_>>().join(" ");
        if start.is_empty() {
            continue;
        }
        procs.push(Proc {
            pid,
            ppid,
            pgrp,
            start,
        });
    }
    if procs.is_empty() {
        return Err("ps returned no processes".to_owned());
    }
    Ok(procs)
}

fn leaf_order(members: &BTreeMap<u32, String>, procs: &[Proc]) -> Vec<u32> {
    let ppid: BTreeMap<u32, u32> = procs.iter().map(|proc| (proc.pid, proc.ppid)).collect();
    let mut remaining: BTreeSet<u32> = members.keys().copied().collect();
    let mut order = Vec::new();
    while !remaining.is_empty() {
        let leaves: Vec<u32> = remaining
            .iter()
            .copied()
            .filter(|pid| !remaining.iter().any(|other| ppid.get(other) == Some(pid)))
            .collect();
        if leaves.is_empty() {
            order.extend(remaining.iter().copied());
            break;
        }
        for pid in leaves {
            remaining.remove(&pid);
            order.push(pid);
        }
    }
    order
}

fn signal_target(kill: &Path, target: &str) -> Result<(), String> {
    let negative = target.starts_with('-');
    let digits = if negative { &target[1..] } else { target };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("refusing signal target {target}"));
    }
    let number: u32 = digits
        .parse()
        .map_err(|_| format!("refusing signal target {target}"))?;
    if number <= 1 {
        return Err(format!("refusing signal target {target}"));
    }
    Command::new(kill)
        .args(["-KILL", target])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|_| ())
        .map_err(|error| format!("kill {target}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_kill_and_ps_are_fixed_absolute_programs() {
        for name in ["kill", "ps"] {
            let path = trusted_tool(name).unwrap_or_else(|error| panic!("{name}: {error}"));
            let text = path.display().to_string();
            assert!(
                text == format!("/bin/{name}") || text == format!("/usr/bin/{name}"),
                "{text}"
            );
        }
        assert!(trusted_tool("not-a-real-tool").is_err());
        assert!(trusted_tool("../kill").is_err());
        assert!(trusted_tool("kill/extra").is_err());
        assert!(trusted_tool("").is_err());
        assert!(trusted_tool("-1").is_err());
    }
}
