//! Direct-child harness. Signals stay inside the process group this process created.
//! A descendant that is still alive after the direct child has exited is an escape.

use std::fs::{self, File};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use std::os::unix::process::CommandExt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcSnap {
    pub pid: u32,
    pub starttime: u64,
    pub state: char,
}

#[derive(Debug)]
pub struct Armed {
    pub child: Child,
    pub pid: u32,
    pub pgid: u32,
    pub starttime: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopReport {
    pub basis: &'static str,
    pub signaled: bool,
    pub force_killed: bool,
    pub escaped: Vec<ProcSnap>,
}

pub fn argv_ok(argv: &[String]) -> bool {
    (1..=8).contains(&argv.len())
        && argv.iter().all(|arg| (1..=256).contains(&arg.len()) && !arg.contains('\0'))
        && argv[0].starts_with('/')
        && !argv[0].contains("..")
}

pub fn spawn(argv: &[String], checkpoint_dir: &Path) -> Result<Armed, &'static str> {
    if !argv_ok(argv) {
        return Err("argv");
    }
    let stdout_path = checkpoint_dir.join("child.stdout");
    let stderr_path = checkpoint_dir.join("child.stderr");
    let stdout = File::create(&stdout_path).map_err(|_| "stdio")?;
    let stderr = File::create(&stderr_path).map_err(|_| "stdio")?;
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .process_group(0)
        .current_dir(checkpoint_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let child = command.spawn().map_err(|_| "spawn")?;
    let pid = child.id();
    let snap = proc_snap(pid).ok_or("proc")?;
    if snap.pid != pid {
        return Err("proc");
    }
    Ok(Armed {
        child,
        pid,
        pgid: pid,
        starttime: snap.starttime,
    })
}

pub fn stop(armed: &mut Armed, budget: Duration) -> StopReport {
    if fs::read_dir("/proc").is_err() {
        return StopReport {
            basis: "unavailable",
            signaled: false,
            force_killed: false,
            escaped: Vec::new(),
        };
    }
    let started = Instant::now();
    let deadline = started + budget;
    let mut known = Vec::new();
    let settle = Instant::now() + Duration::from_millis(300);
    loop {
        remember(&mut known, descendants(armed.pid));
        if !known.is_empty() || Instant::now() >= settle {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let signaled = signal(i32::try_from(armed.pgid).map(|value| -value).unwrap_or(i32::MAX), "TERM");
    loop {
        remember(&mut known, descendants(armed.pid));
        match armed.child.try_wait() {
            Ok(Some(_)) => return finish(signaled, false, &known),
            Ok(None) => {}
            Err(_) => {
                return StopReport {
                    basis: "unavailable",
                    signaled,
                    force_killed: false,
                    escaped: Vec::new(),
                };
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let force = signal(i32::try_from(armed.pgid).map(|value| -value).unwrap_or(i32::MAX), "KILL");
    let _ = armed.child.kill();
    let reap_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match armed.child.try_wait() {
            Ok(Some(_)) => return finish(signaled, force, &known),
            Ok(None) if Instant::now() >= reap_deadline => match still_alive(&known) {
                Ok(escaped) => {
                    return StopReport {
                        basis: "timeout",
                        signaled,
                        force_killed: force,
                        escaped,
                    };
                }
                Err(()) => {
                    return StopReport {
                        basis: "unavailable",
                        signaled,
                        force_killed: force,
                        escaped: Vec::new(),
                    };
                }
            },
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => {
                return StopReport {
                    basis: "unavailable",
                    signaled,
                    force_killed: force,
                    escaped: Vec::new(),
                };
            }
        }
    }
}

pub fn observe_recorded(pid: u32, starttime: u64) -> &'static str {
    if fs::metadata("/proc").is_err() {
        return "unavailable";
    }
    match stat_text(pid) {
        Err(StatErr::Gone) => "absent",
        Err(StatErr::Unreadable) => "unavailable",
        Ok(text) => match parse_snap(pid, &text) {
            Some(snap) if snap.starttime == starttime && snap.state != 'Z' => "alive",
            Some(_) => "absent",
            None => "unavailable",
        },
    }
}

fn remember(known: &mut Vec<ProcSnap>, found: Vec<ProcSnap>) {
    for snap in found {
        if !known.iter().any(|item| item.pid == snap.pid && item.starttime == snap.starttime) {
            known.push(snap);
        }
    }
}

fn finish(signaled: bool, force_killed: bool, known: &[ProcSnap]) -> StopReport {
    match still_alive(known) {
        Ok(escaped) => StopReport {
            basis: if escaped.is_empty() { "stopped" } else { "escaped" },
            signaled,
            force_killed,
            escaped,
        },
        Err(()) => StopReport {
            basis: "unavailable",
            signaled,
            force_killed,
            escaped: Vec::new(),
        },
    }
}

fn still_alive(known: &[ProcSnap]) -> Result<Vec<ProcSnap>, ()> {
    let mut alive = Vec::new();
    for snap in known {
        let text = match stat_text(snap.pid) {
            Ok(text) => text,
            Err(StatErr::Gone) => continue,
            Err(StatErr::Unreadable) => return Err(()),
        };
        let Some(current) = parse_snap(snap.pid, &text) else {
            return Err(());
        };
        if current.starttime == snap.starttime && current.state != 'Z' {
            alive.push(current);
        }
    }
    Ok(alive)
}

fn descendants(parent: u32) -> Vec<ProcSnap> {
    let mut pids = read_children_file(parent);
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            if proc_snap(pid).is_some() && ppid_of(pid) == Some(parent) && !pids.contains(&pid) {
                pids.push(pid);
            }
        }
    }
    pids.into_iter().filter_map(proc_snap).collect()
}

fn read_children_file(pid: u32) -> Vec<u32> {
    let path = format!("/proc/{pid}/task/{pid}/children");
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.split_whitespace().filter_map(|item| item.parse().ok()).collect()
}

fn ppid_of(pid: u32) -> Option<u32> {
    let text = stat_text(pid).ok()?;
    let rest = text.rsplit_once(')')?.1.trim_start();
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    fields.next()?.parse().ok()
}

enum StatErr {
    Gone,
    Unreadable,
}

fn stat_text(pid: u32) -> Result<String, StatErr> {
    match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(StatErr::Gone),
        Err(_) => Err(StatErr::Unreadable),
    }
}

pub fn proc_snap(pid: u32) -> Option<ProcSnap> {
    parse_snap(pid, &stat_text(pid).ok()?)
}

fn parse_snap(pid: u32, text: &str) -> Option<ProcSnap> {
    let rest = text.rsplit_once(')')?.1.trim_start();
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let state = fields.first()?.chars().next()?;
    let starttime = fields.get(19)?.parse().ok()?;
    Some(ProcSnap { pid, starttime, state })
}

fn signal(target: i32, name: &str) -> bool {
    if name != "TERM" && name != "KILL" {
        return false;
    }
    Command::new("/bin/bash")
        .args(["-c", "kill -s \"$1\" -- \"$2\"", "kill", name, &target.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
