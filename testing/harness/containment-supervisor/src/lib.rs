#![forbid(unsafe_code)]

//! Per-command containment supervisor.
//!
//! This process is the child subreaper for one payload. The xtask runner is
//! not a subreaper and does not scan the machine by process name. Cleanup
//! kills only this process's direct children, checked by pid, ppid, and
//! starttime. `comm` is parsed so a hostile command name cannot shift the
//! stat fields. It is not a kill key.
//!
//! `waitpid` returning `ECHILD` means this process had no direct children at
//! that instant. It does not see a grandchild whose parent is still alive, and
//! one sample can race a fork that has not been reparented yet. Three
//! consecutive `ECHILD` results are not the proof. A pass requires a complete
//! `/proc/self/task/*/children` snapshot with no pids and `ECHILD`, twice in a
//! row, with `EINTR` and a truncated snapshot resetting the count. That is
//! still not host isolation. A cgroup is reported only when this process could
//! create one. `cgroup=0` is the normal result under `--cap-drop ALL`. runsc
//! is not this program.

use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const MAX_STATUS_NOTE: usize = 64;
const CHILDREN_BUF: usize = 64 * 1024;
const SETTLE_PASSES: u32 = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Settle {
    Reset,
    Hold,
    Settled,
}

/// Advance the settle counter from one observation.
///
/// `children_complete` is false when the children file was truncated or could
/// not be read. `wait_kind` is `echild` only for a real `ECHILD`, not `EINTR`.
pub(crate) fn settle_step(children_complete: bool, child_count: usize, echild: bool, held: u8) -> (Settle, u8) {
    if !children_complete || child_count > 0 || !echild {
        return (Settle::Reset, 0);
    }
    let next = held.saturating_add(1);
    if next >= 2 {
        (Settle::Settled, next)
    } else {
        (Settle::Hold, next)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProcStat {
    pub comm: String,
    pub state: char,
    pub ppid: i32,
    pub starttime: u64,
}

/// Parse `/proc/<pid>/stat` without reading argv or the environment.
pub(crate) fn parse_stat(text: &str) -> Option<ProcStat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close <= open {
        return None;
    }
    let comm = text[open + 1..close].to_owned();
    if comm.contains('\n') || comm.contains('\0') {
        return None;
    }
    let fields: Vec<&str> = text[close + 1..].split_whitespace().collect();
    if fields.len() < 20 {
        return None;
    }
    let state = fields[0].chars().next()?;
    let ppid: i32 = fields[1].parse().ok()?;
    let starttime: u64 = fields[19].parse().ok()?;
    Some(ProcStat {
        comm,
        state,
        ppid,
        starttime,
    })
}

pub fn run() -> i32 {
    let args: Vec<String> = env::args().skip(1).collect();
    #[cfg(target_os = "linux")]
    {
        linux::run(&args)
    }
    #[cfg(not(target_os = "linux"))]
    {
        refuse_platform(&args)
    }
}

#[cfg(not(target_os = "linux"))]
fn refuse_platform(args: &[String]) -> i32 {
    if let Some(status) = supervise_status_path(args) {
        write_status(
            Path::new(status),
            "unavailable",
            false,
            -1,
            true,
            0,
            false,
            "platform",
            false,
        );
    }
    81
}

#[cfg(not(target_os = "linux"))]
fn supervise_status_path(args: &[String]) -> Option<&str> {
    if args.first().is_some_and(|arg| arg == "--launch") || args.len() < 5 || args[4] != "--" {
        return None;
    }
    Some(args[0].as_str())
}

#[allow(clippy::too_many_arguments)]
fn write_status(
    path: &Path,
    containment: &str,
    spawned: bool,
    exit_code: i32,
    cleanup_incomplete: bool,
    direct_pid: u32,
    output_limited: bool,
    note: &str,
    cgroup: bool,
) {
    let note = sanitize_note(note);
    let body = format!(
        "containment={containment}\nspawned={spawned}\nexit_code={exit_code}\ncleanup_incomplete={cleanup}\ndirect_pid={direct_pid}\noutput_limited={limited}\nnote={note}\ncgroup={cgroup}\n",
        spawned = if spawned { 1 } else { 0 },
        cleanup = if cleanup_incomplete { 1 } else { 0 },
        limited = if output_limited { 1 } else { 0 },
        cgroup = if cgroup { 1 } else { 0 },
    );
    let Some(parent) = path.parent() else {
        return;
    };
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|name| name.to_str()).unwrap_or("status")
    ));
    let mut file = match File::create(&tmp) {
        Ok(file) => file,
        Err(_) => return,
    };
    if file.write_all(body.as_bytes()).is_err() {
        let _ = fs::remove_file(&tmp);
        return;
    }
    let _ = file.sync_all();
    drop(file);
    if fs::rename(&tmp, path).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

fn sanitize_note(note: &str) -> &str {
    if note.is_empty()
        || note.len() > MAX_STATUS_NOTE
        || note.bytes().any(|byte| !(byte.is_ascii_alphanumeric() || byte == b'-'))
    {
        "note-rejected"
    } else {
        note
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::os::unix::process::CommandExt;

    use nix::errno::Errno;
    use nix::sys::prctl::{set_child_subreaper, set_pdeathsig};
    use nix::sys::signal::{kill, killpg, sigprocmask, SigSet, SigmaskHow, Signal};
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
    use nix::unistd::{execvp, getpid, getppid, Pid};
    use std::ffi::CString;

    pub(super) fn run(args: &[String]) -> i32 {
        if args.first().is_some_and(|arg| arg == "--launch") {
            return launch(&args[1..]);
        }
        let Some(job) = parse_job(args) else {
            if let Some(status) = args.first()
                && args.len() >= 5
            {
                write_status(
                    Path::new(status),
                    "unavailable",
                    false,
                    -1,
                    true,
                    0,
                    false,
                    "protocol-malformed",
                    false,
                );
            }
            return 82;
        };
        supervise(job)
    }

    struct Job {
        status: String,
        stdout: String,
        stderr: String,
        limit: u64,
        command: Vec<String>,
    }

    fn parse_job(args: &[String]) -> Option<Job> {
        if args.len() < 6 || args[4] != "--" {
            return None;
        }
        let limit: u64 = args[3].parse().ok()?;
        if limit == 0 {
            return None;
        }
        let command = args[5..].to_vec();
        if command.is_empty() || command.iter().any(String::is_empty) {
            return None;
        }
        Some(Job {
            status: args[0].clone(),
            stdout: args[1].clone(),
            stderr: args[2].clone(),
            limit,
            command,
        })
    }

    fn launch(args: &[String]) -> i32 {
        let Some(split) = args.iter().position(|arg| arg == "--") else {
            return 82;
        };
        let command = &args[split + 1..];
        if command.is_empty() {
            return 82;
        }
        if set_pdeathsig(Some(Signal::SIGKILL)).is_err() || getppid().as_raw() == 1 {
            return 79;
        }
        let all = SigSet::all();
        let _ = sigprocmask(SigmaskHow::SIG_UNBLOCK, Some(&all), None);
        let mut argv = Vec::with_capacity(command.len());
        for arg in command {
            match CString::new(arg.as_str()) {
                Ok(item) => argv.push(item),
                Err(_) => return 127,
            }
        }
        let _ = execvp(&argv[0], &argv);
        127
    }

    #[allow(clippy::cognitive_complexity)]
    fn supervise(job: Job) -> i32 {
        let status = Path::new(&job.status);
        let stop = Arc::new(AtomicBool::new(false));
        if let Err(note) = arm_supervisor(Arc::clone(&stop)) {
            write_status(status, "unavailable", false, -1, true, 0, false, note, false);
            return 81;
        }
        let cgroup = arm_cgroup();
        let stdout = match File::create(&job.stdout) {
            Ok(file) => file,
            Err(_) => {
                write_status(status, "unavailable", false, -1, true, 0, false, "capture-failed", false);
                return 81;
            }
        };
        let stderr = match File::create(&job.stderr) {
            Ok(file) => file,
            Err(_) => {
                write_status(status, "unavailable", false, -1, true, 0, false, "capture-failed", false);
                return 81;
            }
        };
        let exe = match env::current_exe() {
            Ok(path) => path,
            Err(_) => {
                write_status(status, "unavailable", false, -1, true, 0, false, "launch-failed", false);
                return 81;
            }
        };
        let mut child = match Command::new(&exe)
            .arg("--launch")
            .arg("--")
            .args(&job.command)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => {
                write_status(status, "unavailable", false, -1, true, 0, false, "launch-failed", false);
                return 81;
            }
        };
        let direct = child.id();
        let direct_start = read_identity(direct).ok().map(|stat| stat.starttime);
        let mut cgroup_on = false;
        if let Some(dir) = cgroup.as_ref() {
            if write_cgroup_proc(dir, direct) {
                cgroup_on = true;
            } else {
                let _ = fs::remove_dir(dir);
            }
        }
        let Some(out_pipe) = child.stdout.take() else {
            stop_direct(direct, direct_start);
            write_status(status, "unverified", true, -1, true, direct, false, "capture-failed", cgroup_on);
            return 0;
        };
        let Some(err_pipe) = child.stderr.take() else {
            stop_direct(direct, direct_start);
            write_status(status, "unverified", true, -1, true, direct, false, "capture-failed", cgroup_on);
            return 0;
        };
        let limited_out = Arc::new(AtomicBool::new(false));
        let limited_err = Arc::new(AtomicBool::new(false));
        let out_thread = pump(out_pipe, stdout, job.limit, Arc::clone(&limited_out));
        let err_thread = pump(err_pipe, stderr, job.limit, Arc::clone(&limited_err));
        let mut collected = None;
        loop {
            if stop.load(Ordering::Relaxed)
                || limited_out.load(Ordering::Relaxed)
                || limited_err.load(Ordering::Relaxed)
            {
                break;
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    collected = Some(status.code().unwrap_or(-1));
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(_) => break,
            }
        }
        // Nix reaps any zombie that try_wait did not collect. Drop must not
        // block in a second wait on a child this process has already reaped.
        std::mem::forget(child);
        // Kill before joining so a held pipe reaches EOF. The limit flag is
        // read after the join: a child can exit while bytes are still queued.
        let mut note = if stop.load(Ordering::Relaxed) { "stopped" } else { "ok" };
        let cgroup_path = if cgroup_on { cgroup.as_deref() } else { None };
        let direct_code = kill_and_settle(direct, direct_start, collected, cgroup_path, &mut note);
        join_bounded(out_thread);
        join_bounded(err_thread);
        let output_limited = limited_out.load(Ordering::Relaxed) || limited_err.load(Ordering::Relaxed);
        if output_limited && !note_fails(note) {
            note = "output-limit";
        }
        let _ = sync_path(&job.stdout);
        let _ = sync_path(&job.stderr);
        let cleanup_incomplete = note_fails(note);
        let containment = if cleanup_incomplete { "unverified" } else { "verified" };
        if let Some(dir) = cgroup.as_ref().filter(|_| cgroup_on) {
            let _ = fs::remove_dir(dir);
        }
        write_status(
            status,
            containment,
            true,
            direct_code,
            cleanup_incomplete,
            direct,
            output_limited,
            note,
            cgroup_on,
        );
        0
    }

    fn join_bounded(handle: thread::JoinHandle<()>) {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = handle.join();
            let _ = sender.send(());
        });
        let _ = receiver.recv_timeout(Duration::from_millis(500));
    }

    fn note_fails(note: &str) -> bool {
        !matches!(note, "ok" | "stopped" | "output-limit")
    }

    fn arm_supervisor(stop: Arc<AtomicBool>) -> Result<(), &'static str> {
        let mut blocked = SigSet::empty();
        blocked.add(Signal::SIGTERM);
        blocked.add(Signal::SIGINT);
        blocked.add(Signal::SIGPIPE);
        sigprocmask(SigmaskHow::SIG_BLOCK, Some(&blocked), None).map_err(|_| "subreaper-unavailable")?;
        let wait_set = blocked;
        thread::spawn(move || {
            while let Ok(signal) = wait_set.wait() {
                if signal == Signal::SIGTERM || signal == Signal::SIGINT {
                    stop.store(true, Ordering::Relaxed);
                    break;
                }
            }
        });
        set_child_subreaper(true).map_err(|_| "subreaper-unavailable")?;
        set_pdeathsig(Some(Signal::SIGTERM)).map_err(|_| "subreaper-unavailable")?;
        if getppid().as_raw() == 1 {
            return Err("parent-dead");
        }
        Ok(())
    }

    fn pump(
        mut pipe: impl Read + Send + 'static,
        mut file: File,
        limit: u64,
        limited: Arc<AtomicBool>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut kept = 0u64;
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) => break,
                    Ok(count) => {
                        if kept < limit {
                            let room = (limit - kept) as usize;
                            let take = count.min(room);
                            if take > 0 && file.write_all(&buf[..take]).is_err() {
                                limited.store(true, Ordering::Relaxed);
                                break;
                            }
                            kept += take as u64;
                            if count > take {
                                limited.store(true, Ordering::Relaxed);
                            }
                        } else {
                            limited.store(true, Ordering::Relaxed);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        })
    }

    fn sync_path(path: &str) -> std::io::Result<()> {
        let file = File::options().write(true).open(path)?;
        file.sync_all()
    }

    #[allow(clippy::cognitive_complexity)]
    fn kill_and_settle(
        direct: u32,
        direct_start: Option<u64>,
        collected: Option<i32>,
        cgroup: Option<&Path>,
        note: &mut &'static str,
    ) -> i32 {
        let mut direct_code = collected;
        let mut why = None;
        let mut held = 0u8;
        let mut settled = false;
        for _ in 0..SETTLE_PASSES {
            if let Some(reason) = signal_owned(direct, direct_start) {
                why = Some(reason);
            }
            if let Some(dir) = cgroup
                && fs::write(dir.join("cgroup.kill"), b"1\n").is_err()
            {
                why = Some("cgroup-populated");
            }
            let (complete, count) = match read_children() {
                Ok(pids) => (true, pids.len()),
                Err(()) => (false, 1),
            };
            match reap_once(direct, &mut direct_code) {
                Reap::Echild => {
                    let (step, next) = settle_step(complete, count, true, held);
                    held = next;
                    if step == Settle::Settled && confirm_empty(direct, &mut direct_code) {
                        if let Some(dir) = cgroup {
                            match cgroup_populated(dir) {
                                Ok(false) => {
                                    settled = true;
                                    break;
                                }
                                _ => {
                                    why = Some("cgroup-populated");
                                    held = 0;
                                }
                            }
                        } else {
                            settled = true;
                            break;
                        }
                    }
                }
                Reap::Live => held = 0,
                Reap::Error => {
                    held = 0;
                    why = Some("reap-timeout");
                }
            }
            if !complete {
                held = 0;
                why = Some("list-unreadable");
            }
            thread::sleep(Duration::from_millis(20));
        }
        if !settled {
            *note = why.unwrap_or("reap-timeout");
        }
        direct_code.unwrap_or(-1)
    }

    fn confirm_empty(direct: u32, direct_code: &mut Option<i32>) -> bool {
        let Ok(pids) = read_children() else {
            return false;
        };
        if !pids.is_empty() {
            return false;
        }
        matches!(reap_once(direct, direct_code), Reap::Echild)
    }

    enum Reap {
        Echild,
        Live,
        Error,
    }

    fn reap_once(direct: u32, direct_code: &mut Option<i32>) -> Reap {
        let mut reaped = 0u32;
        loop {
            match waitpid(None, Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::StillAlive) => return Reap::Live,
                Ok(status) => {
                    reaped += 1;
                    if reaped > 10_000 {
                        return Reap::Live;
                    }
                    if let Some(pid) = status.pid()
                        && i32::try_from(direct).ok() == Some(pid.as_raw())
                        && direct_code.is_none()
                    {
                        *direct_code = Some(exit_code_of(status));
                    }
                }
                Err(Errno::ECHILD) => return Reap::Echild,
                Err(Errno::EINTR) => continue,
                Err(_) => return Reap::Error,
            }
        }
    }

    fn exit_code_of(status: WaitStatus) -> i32 {
        match status {
            WaitStatus::Exited(_, code) => code,
            _ => -1,
        }
    }

    fn kill_pid(pid: u32) -> bool {
        let Some(target) = pid_of(pid) else {
            return false;
        };
        let _ = killpg(target, Signal::SIGKILL);
        match kill(target, Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH) => true,
            Err(_) => false,
        }
    }

    fn stop_direct(direct: u32, direct_start: Option<u64>) {
        if let Identity::Ours(start) = identity(direct)
            && (direct_start.is_none() || direct_start == Some(start))
        {
            let _ = kill_pid(direct);
        }
    }

    fn signal_owned(direct: u32, direct_start: Option<u64>) -> Option<&'static str> {
        let mut uncertain = false;
        let mut failed = false;
        // The direct pid is signaled only when both stat reads still show it
        // is our child and its starttime is the one recorded at spawn.
        // A reused pid is left alone. `comm` is not a kill key.
        match identity(direct) {
            Identity::Ours(start) if direct_start == Some(start) => {
                if !kill_pid(direct) {
                    failed = true;
                }
            }
            Identity::Gone => {}
            Identity::Ours(_) | Identity::NotOurs | Identity::Unreadable => {
                // The pid was reused or is not our child. Leave it alone.
            }
        }
        let pids = match read_children() {
            Ok(pids) => pids,
            Err(()) => return Some("list-unreadable"),
        };
        for pid in pids {
            match identity(pid) {
                Identity::Ours(_) => {
                    if !kill_pid(pid) {
                        failed = true;
                    }
                }
                Identity::Gone => {}
                Identity::NotOurs | Identity::Unreadable => uncertain = true,
            }
        }
        if uncertain {
            Some("identity-uncertain")
        } else if failed {
            Some("signal-failed")
        } else {
            None
        }
    }

    enum Identity {
        Ours(u64),
        Gone,
        NotOurs,
        Unreadable,
    }

    fn read_identity(pid: u32) -> Result<ProcStat, Identity> {
        if pid <= 1 {
            return Err(Identity::NotOurs);
        }
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(text) => parse_stat(&text).ok_or(Identity::Unreadable),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Identity::Gone),
            Err(_) => Err(Identity::Unreadable),
        }
    }

    fn identity(pid: u32) -> Identity {
        let first = match read_identity(pid) {
            Ok(stat) => stat,
            Err(kind) => return kind,
        };
        let second = match read_identity(pid) {
            Ok(stat) => stat,
            Err(Identity::Gone) => return Identity::Gone,
            Err(kind) => return kind,
        };
        // A reused pid has a different starttime. Do not signal it.
        // `comm` is parsed only so a parenthesis in the name cannot move ppid.
        if first.starttime != second.starttime || first.ppid != second.ppid || first.comm != second.comm {
            return Identity::Unreadable;
        }
        if first.ppid == getpid().as_raw() {
            Identity::Ours(first.starttime)
        } else {
            Identity::NotOurs
        }
    }

    fn pid_of(pid: u32) -> Option<Pid> {
        let raw = i32::try_from(pid).ok()?;
        if raw <= 1 {
            None
        } else {
            Some(Pid::from_raw(raw))
        }
    }

    fn read_children() -> Result<Vec<u32>, ()> {
        let mut found = Vec::new();
        let tasks = fs::read_dir("/proc/self/task").map_err(|_| ())?;
        for task in tasks {
            let task = task.map_err(|_| ())?;
            let path = task.path().join("children");
            let mut file = match File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(()),
            };
            let mut buf = vec![0u8; CHILDREN_BUF];
            let count = file.read(&mut buf).map_err(|_| ())?;
            if count == CHILDREN_BUF {
                return Err(());
            }
            let text = String::from_utf8_lossy(&buf[..count]);
            for token in text.split_whitespace() {
                let pid: u32 = token.parse().map_err(|_| ())?;
                if pid > 1 {
                    found.push(pid);
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        Ok(found)
    }

    fn arm_cgroup() -> Option<std::path::PathBuf> {
        let text = fs::read_to_string("/proc/self/cgroup").ok()?;
        let rel = text.lines().find_map(|line| line.strip_prefix("0::"))?;
        if rel.is_empty() || rel.contains('\0') {
            return None;
        }
        let dir = std::path::PathBuf::from(format!("/sys/fs/cgroup{rel}/oo-xtask-{}", getpid().as_raw()));
        if fs::create_dir(&dir).is_err() {
            return None;
        }
        let procs = dir.join("cgroup.procs");
        let kill = dir.join("cgroup.kill");
        if !procs.is_file() || !kill.is_file() {
            let _ = fs::remove_dir(&dir);
            return None;
        }
        Some(dir)
    }

    fn write_cgroup_proc(dir: &Path, pid: u32) -> bool {
        fs::write(dir.join("cgroup.procs"), format!("{pid}\n")).is_ok()
    }

    fn cgroup_populated(dir: &Path) -> Result<bool, ()> {
        let text = fs::read_to_string(dir.join("cgroup.events")).map_err(|_| ())?;
        for line in text.lines() {
            if let Some(value) = line.strip_prefix("populated ") {
                return match value.trim() {
                    "0" => Ok(false),
                    "1" => Ok(true),
                    _ => Err(()),
                };
            }
        }
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_echild_does_not_settle_and_a_child_resets() {
        assert_eq!(settle_step(true, 0, true, 0), (Settle::Hold, 1));
        assert_eq!(settle_step(true, 0, true, 1), (Settle::Settled, 2));
        assert_eq!(settle_step(true, 1, true, 1), (Settle::Reset, 0));
        assert_eq!(settle_step(false, 0, true, 1), (Settle::Reset, 0));
        assert_eq!(settle_step(true, 0, false, 1), (Settle::Reset, 0));
    }

    #[test]
    fn stat_parser_uses_the_last_paren_and_rejects_a_short_line() {
        let stat = parse_stat("42 (sleep) S 7 42 42 0 0 0 0 0 0 0 1 2 3 4 20 0 1 0 99991 1 2").unwrap();
        assert_eq!(stat.comm, "sleep");
        assert_eq!(stat.state, 'S');
        assert_eq!(stat.ppid, 7);
        assert_eq!(stat.starttime, 99991);
        let tricky = parse_stat("9 (we)ird) S 3 9 9 0 0 0 0 0 0 0 0 0 0 0 20 0 1 0 50").unwrap();
        assert_eq!(tricky.comm, "we)ird");
        assert_eq!(tricky.ppid, 3);
        assert_eq!(tricky.starttime, 50);
        assert!(parse_stat("1 (x) S 1\n").is_none());
    }

    #[test]
    fn notes_reject_newlines_and_spaces() {
        assert_eq!(sanitize_note("ok"), "ok");
        assert_eq!(sanitize_note("output-limit"), "output-limit");
        assert_eq!(sanitize_note("bad note"), "note-rejected");
        assert_eq!(sanitize_note("a\nb"), "note-rejected");
    }
}
