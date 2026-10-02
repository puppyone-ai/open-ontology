//! Per-command ownership for a spawned process.
//!
//! The supervisor is a prebuilt executable, one process per command. It is the
//! child subreaper for that command only. This runner does not call
//! `PR_SET_CHILD_SUBREAPER`, does not compile a helper at runtime, and does not
//! scan the machine for process names.
//!
//! On Linux the helper's proof is a complete empty `/proc/self/task/*/children`
//! snapshot together with `waitpid` returning `ECHILD`, observed twice, then
//! confirmed once more. `EINTR` and a truncated snapshot reset the count.
//! Three `ECHILD` results by themselves are not the proof: a grandchild whose
//! parent is still alive is invisible, and a fork can land between samples.
//! That remaining race is not host isolation, a pid namespace, or runsc.
//! `cgroup=0` means this process could not arm a cgroup. It is expected under
//! `--cap-drop ALL` and does not by itself fail a settled reap. An armed
//! cgroup must be empty. runsc is a separate, unsupported capability.
//!
//! The helper path is the checked-in artifact. Its sha256 must match the
//! embedded pin and `provenance/artifact.sha256`. The file must be a regular
//! owner-executable, not a symlink, and not group or world writable. The
//! runner copies those bytes to a private 0700 file, hashes the copy, and
//! executes the copy. A same-uid replacement between that hash and `exec` is
//! still possible. Imported captures are not a helper and are not executed.
//!
//! macOS has no unprivileged subreaper in this slice. The runner returns
//! `containment=unavailable` and `note=platform` without executing the Linux
//! artifact. A process-group sample is not that primitive.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

use crate::owned_process::trusted_tool;
use crate::util::sha256_hex;

pub const VERIFIED: &str = "verified";
pub const UNAVAILABLE: &str = "unavailable";
pub const UNVERIFIED: &str = "unverified";

const REAP_WAIT: Duration = Duration::from_millis(2000);

/// Sha256 of `testing/harness/containment-supervisor/bin/containment-supervisor`.
/// The provenance file must contain this same line. A zero digest matches no
/// built artifact.
const SUPERVISOR_SHA256: &str = "3569f4e45f88cdedef55c6eb5b23e2c4eb9e9dc1c14690085ddf72681a95628e";

static STAGE_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HelperFault {
    Missing,
    Mismatch,
    Untrusted,
}

fn fault_note(fault: HelperFault) -> &'static str {
    match fault {
        HelperFault::Missing => "helper-missing",
        HelperFault::Mismatch => "helper-mismatch",
        HelperFault::Untrusted => "helper-untrusted",
    }
}

pub(crate) fn artifact_path() -> PathBuf {
    crate::util::repo_root().join(
        "testing/harness/containment-supervisor/bin/containment-supervisor",
    )
}

fn provenance_path() -> PathBuf {
    crate::util::repo_root().join(
        "testing/harness/containment-supervisor/provenance/artifact.sha256",
    )
}

#[derive(Clone, Debug)]
pub struct ContainedRun {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub output_limited: bool,
    pub cleanup_incomplete: bool,
    pub containment: String,
    pub spawned: bool,
    pub note: String,
    pub cgroup: bool,
    pub direct_pid: u32,
    pub spawn_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Status {
    containment: String,
    spawned: bool,
    exit_code: i32,
    cleanup_incomplete: bool,
    output_limited: bool,
    note: String,
    cgroup: bool,
    direct_pid: u32,
}

pub fn blocks_pass(containment: &str, spawned: bool, cleanup_incomplete: bool, note: &str) -> bool {
    containment != VERIFIED || !spawned || cleanup_incomplete || note_blocks(note)
}

fn note_blocks(note: &str) -> bool {
    matches!(
        note,
        "platform"
            | "subreaper-unavailable"
            | "pipe-failed"
            | "capture-failed"
            | "fork-failed"
            | "reap-timeout"
            | "signal-failed"
            | "identity-uncertain"
            | "list-unreadable"
            | "cgroup-populated"
            | "parent-dead"
            | "supervisor-killed"
            | "status-missing"
            | "helper-missing"
            | "helper-mismatch"
            | "helper-untrusted"
            | "launch-failed"
            | "protocol-malformed"
    )
}

impl ContainedRun {
    fn unavailable(error: String) -> Self {
        Self {
            exit_code: -1,
            stdout: Vec::new(),
            stderr: error.as_bytes().to_vec(),
            timed_out: false,
            output_limited: false,
            cleanup_incomplete: true,
            containment: UNAVAILABLE.to_owned(),
            spawned: false,
            note: "unavailable".to_owned(),
            cgroup: false,
            direct_pid: 0,
            spawn_error: Some(if error.contains("containment") {
                error
            } else {
                format!("containment unavailable: {error}")
            }),
        }
    }
}

fn refused(note: &'static str) -> ContainedRun {
    let error = format!("containment unavailable: {note}");
    ContainedRun {
        exit_code: -1,
        stdout: Vec::new(),
        stderr: error.as_bytes().to_vec(),
        timed_out: false,
        output_limited: false,
        cleanup_incomplete: true,
        containment: UNAVAILABLE.to_owned(),
        spawned: false,
        note: note.to_owned(),
        cgroup: false,
        direct_pid: 0,
        spawn_error: Some(error),
    }
}

pub fn supervise(
    command: &Command,
    deadline: Duration,
    stdout_path: &Path,
    stderr_path: &Path,
    status_path: &Path,
    limit: u64,
) -> ContainedRun {
    if !cfg!(target_os = "linux") {
        return refused("platform");
    }
    let argv = command_argv(command);
    if argv.first().is_none_or(|program| program.is_empty()) {
        return ContainedRun::unavailable("coordinator has no command to execute".to_owned());
    }
    let digest = match expected_digest() {
        Ok(digest) => digest,
        Err(fault) => return refused(fault_note(fault)),
    };
    if let Err(fault) = assess_file(&artifact_path(), &digest) {
        return refused(fault_note(fault));
    }
    let _ = fs::remove_file(status_path);
    let (mut child, staged) = match launch_supervisor(
        &digest,
        status_path,
        stdout_path,
        stderr_path,
        limit,
        command,
        &argv,
    ) {
        Ok(launched) => launched,
        Err(run) => return *run,
    };
    let _remove_staged = RemoveFile(staged);
    let started = Instant::now();
    let mut timed_out = false;
    let wait_result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() >= deadline => {
                timed_out = true;
                let _ = terminate(child.id());
                break wait_after_signal(&mut child);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => break Err(error.to_string()),
        }
    };
    let forced_unverified = wait_result.is_err();
    if forced_unverified {
        let _ = child.kill();
        let _ = child.wait();
    }
    let mut status = read_status(status_path).unwrap_or(Status {
        containment: if forced_unverified {
            UNVERIFIED.to_owned()
        } else {
            UNAVAILABLE.to_owned()
        },
        spawned: false,
        exit_code: -1,
        cleanup_incomplete: true,
        output_limited: false,
        note: "status-missing".to_owned(),
        cgroup: false,
        direct_pid: 0,
    });
    if forced_unverified {
        status.containment = UNVERIFIED.to_owned();
        status.cleanup_incomplete = true;
        status.note = "supervisor-killed".to_owned();
    }
    if note_blocks(&status.note) || status.containment != VERIFIED || status.cleanup_incomplete {
        status.cleanup_incomplete = true;
        if status.containment == VERIFIED {
            status.containment = UNVERIFIED.to_owned();
        }
    }
    let (stdout, stdout_over) = read_prefix(stdout_path, limit as usize);
    let (stderr, stderr_over) = read_prefix(stderr_path, limit as usize);
    let output_limited = status.output_limited || stdout_over || stderr_over;
    let mut exit_code = status.exit_code;
    let cleanup_incomplete = status.cleanup_incomplete || status.containment != VERIFIED;
    if (timed_out || output_limited || cleanup_incomplete) && exit_code == 0 {
        exit_code = -1;
    }
    let spawn_error = if status.spawned {
        None
    } else {
        Some(format!(
            "containment backend is unavailable ({})",
            status.note
        ))
    };
    ContainedRun {
        exit_code,
        stdout,
        stderr: if status.spawned {
            stderr
        } else if stderr.is_empty() {
            spawn_error.clone().unwrap_or_default().into_bytes()
        } else {
            stderr
        },
        timed_out,
        output_limited,
        cleanup_incomplete,
        containment: status.containment,
        spawned: status.spawned,
        note: status.note,
        cgroup: status.cgroup,
        direct_pid: status.direct_pid,
        spawn_error,
    }
}

fn expected_digest() -> Result<String, HelperFault> {
    if SUPERVISOR_SHA256.len() != 64
        || !SUPERVISOR_SHA256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(HelperFault::Mismatch);
    }
    let text = fs::read_to_string(provenance_path()).map_err(|_| HelperFault::Missing)?;
    if text.trim() != SUPERVISOR_SHA256 || text.contains('\0') {
        return Err(HelperFault::Mismatch);
    }
    Ok(SUPERVISOR_SHA256.to_owned())
}

fn current_euid() -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        let text = fs::read_to_string("/proc/self/status").ok()?;
        for line in text.lines() {
            let Some(rest) = line.strip_prefix("Uid:") else {
                continue;
            };
            let mut fields = rest.split_whitespace();
            let _real = fields.next()?;
            return fields.next().and_then(|field| field.parse().ok());
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        let exe = std::env::current_exe().ok()?;
        Some(fs::metadata(exe).ok()?.uid())
    }
}

fn assess_file(path: &Path, expected_digest: &str) -> Result<(), HelperFault> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(HelperFault::Missing);
        }
        Err(_) => return Err(HelperFault::Untrusted),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(HelperFault::Untrusted);
    }
    let mode = metadata.mode();
    if mode & 0o022 != 0 || mode & 0o100 == 0 {
        return Err(HelperFault::Untrusted);
    }
    let uid = metadata.uid();
    let euid = current_euid().ok_or(HelperFault::Untrusted)?;
    if uid != 0 && uid != euid {
        return Err(HelperFault::Untrusted);
    }
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).ok_or(HelperFault::Untrusted)?;
    let parent_meta = fs::symlink_metadata(parent).map_err(|_| HelperFault::Untrusted)?;
    if parent_meta.file_type().is_symlink() {
        return Err(HelperFault::Untrusted);
    }
    let parent_mode = parent_meta.mode();
    if parent_mode & 0o002 != 0 && parent_mode & 0o1000 == 0 {
        return Err(HelperFault::Untrusted);
    }
    let bytes = fs::read(path).map_err(|_| HelperFault::Untrusted)?;
    if sha256_hex(&bytes) != expected_digest {
        return Err(HelperFault::Mismatch);
    }
    Ok(())
}

fn stage_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR")
        && !target.is_empty()
    {
        roots.push(PathBuf::from(target));
    }
    if let Ok(current) = std::env::current_exe()
        && let Some(parent) = current.parent()
    {
        roots.push(parent.to_path_buf());
    }
    roots
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn launch_supervisor(
    digest: &str,
    status_path: &Path,
    stdout_path: &Path,
    stderr_path: &Path,
    limit: u64,
    command: &Command,
    argv: &[String],
) -> Result<(std::process::Child, PathBuf), Box<ContainedRun>> {
    let bytes = match fs::read(artifact_path()) {
        Ok(bytes) if sha256_hex(&bytes) == digest => bytes,
        Ok(_) => return Err(Box::new(refused("helper-mismatch"))),
        Err(_) => return Err(Box::new(refused("helper-missing"))),
    };
    for root in stage_roots() {
        let dir = root.join("oo-xtask-contain");
        if fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
        let seq = STAGE_SEQ.fetch_add(1, Ordering::Relaxed);
        let dest = dir.join(format!("helper-{}-{seq}", std::process::id()));
        if write_private(&dest, &bytes).is_err() {
            let _ = fs::remove_file(&dest);
            continue;
        }
        if assess_file(&dest, digest).is_err() {
            let _ = fs::remove_file(&dest);
            continue;
        }
        let mut supervisor = Command::new(&dest);
        supervisor
            .arg(status_path)
            .arg(stdout_path)
            .arg(stderr_path)
            .arg(limit.to_string())
            .arg("--")
            .args(argv)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(dir) = command.get_current_dir() {
            supervisor.current_dir(dir);
        }
        for (key, value) in command.get_envs() {
            match value {
                Some(value) => {
                    supervisor.env(key, value);
                }
                None => {
                    supervisor.env_remove(key);
                }
            }
        }
        match supervisor.spawn() {
            Ok(child) => return Ok((child, dest)),
            Err(_) => {
                let _ = fs::remove_file(&dest);
            }
        }
    }
    Err(Box::new(refused("launch-failed")))
}

struct RemoveFile(PathBuf);

impl Drop for RemoveFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn command_argv(command: &Command) -> Vec<String> {
    let mut argv = vec![command.get_program().to_string_lossy().into_owned()];
    argv.extend(command.get_args().map(|arg| arg.to_string_lossy().into_owned()));
    argv
}

fn terminate(pid: u32) -> Result<(), String> {
    if pid <= 1 {
        return Err("refusing to signal pid <= 1".to_owned());
    }
    let kill = trusted_tool("kill")?;
    let status = Command::new(kill)
        .args(["-TERM", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("kill -TERM {pid}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "kill -TERM {pid} exited {}",
            status.code().unwrap_or(-1)
        ))
    }
}

fn wait_after_signal(child: &mut std::process::Child) -> Result<std::process::ExitStatus, String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() >= REAP_WAIT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("supervisor did not finish cleanup before it was killed".to_owned());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn read_prefix(path: &Path, limit: usize) -> (Vec<u8>, bool) {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return (Vec::new(), false),
    };
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut limited = false;
    loop {
        use std::io::Read;
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                let room = limit.saturating_sub(kept.len());
                if count > room {
                    kept.extend_from_slice(&chunk[..room]);
                    limited = true;
                    break;
                }
                kept.extend_from_slice(&chunk[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    (kept, limited)
}

fn read_status(path: &Path) -> Option<Status> {
    let text = fs::read_to_string(path).ok()?;
    let mut containment = String::new();
    let mut spawned = None;
    let mut exit_code = None;
    let mut cleanup_incomplete = None;
    let mut output_limited = None;
    let mut note = String::new();
    let mut cgroup = false;
    let mut direct_pid = 0u32;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "containment" => containment = value.to_owned(),
            "spawned" => spawned = value.parse::<u8>().ok().map(|bit| bit == 1),
            "exit_code" => exit_code = value.parse().ok(),
            "cleanup_incomplete" => {
                cleanup_incomplete = value.parse::<u8>().ok().map(|bit| bit == 1)
            }
            "output_limited" => output_limited = value.parse::<u8>().ok().map(|bit| bit == 1),
            "note" => note = value.to_owned(),
            "cgroup" => cgroup = value == "1",
            "direct_pid" => direct_pid = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    if !matches!(containment.as_str(), VERIFIED | UNAVAILABLE | UNVERIFIED) {
        return None;
    }
    Some(Status {
        containment,
        spawned: spawned?,
        exit_code: exit_code?,
        cleanup_incomplete: cleanup_incomplete?,
        output_limited: output_limited?,
        note,
        cgroup,
        direct_pid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status_text(body: &str) -> Option<Status> {
        let path = std::env::temp_dir().join(format!(
            "oo-a082-status-{}-{}",
            std::process::id(),
            sha256_hex(body.as_bytes())
        ));
        fs::write(&path, body).unwrap();
        let parsed = read_status(&path);
        let _ = fs::remove_file(&path);
        parsed
    }

    fn private_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oo-a082-{label}-{}-{}",
            std::process::id(),
            STAGE_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    #[test]
    fn checked_in_supervisor_pin_matches_the_embedded_digest() {
        let text = fs::read_to_string(provenance_path()).expect("artifact pin");
        assert_eq!(text.trim(), SUPERVISOR_SHA256);
        assert_eq!(text.trim().len(), 64);
        let bytes = fs::read(artifact_path()).expect("pinned helper bytes");
        assert_eq!(sha256_hex(&bytes), SUPERVISOR_SHA256);
        let source = fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/containment.rs"),
        )
        .expect("containment source");
        let embedded_c = format!("{}{}", "SUPERVISOR_", "C");
        let include_token = format!("{}{}", "#", "include");
        let compiler_token = format!("{}{}", "trusted_", "compiler");
        assert!(!source.contains(&embedded_c));
        assert!(!source.contains(&include_token));
        assert!(!source.contains(&compiler_token));
    }

    #[test]
    fn missing_tampered_and_untrusted_helpers_fail_closed() {
        let dir = private_dir("trust");
        let missing = dir.join("absent");
        assert_eq!(
            assess_file(&missing, SUPERVISOR_SHA256),
            Err(HelperFault::Missing)
        );
        let file = dir.join("helper");
        fs::write(&file, b"#!/bin/sh\necho substituted\n").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        let digest = sha256_hex(&fs::read(&file).unwrap());
        assert_eq!(assess_file(&file, &digest), Ok(()));
        assert_eq!(
            assess_file(&file, SUPERVISOR_SHA256),
            Err(HelperFault::Mismatch)
        );
        fs::set_permissions(&file, fs::Permissions::from_mode(0o775)).unwrap();
        assert_eq!(assess_file(&file, &digest), Err(HelperFault::Untrusted));
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(assess_file(&file, &digest), Err(HelperFault::Untrusted));
        let link = dir.join("linked");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert_eq!(assess_file(&link, &digest), Err(HelperFault::Untrusted));
        let open = dir.join("open-parent");
        fs::create_dir(&open).unwrap();
        fs::set_permissions(&open, fs::Permissions::from_mode(0o777)).unwrap();
        let inside = open.join("helper");
        fs::write(&inside, b"#!/bin/sh\necho substituted\n").unwrap();
        fs::set_permissions(&inside, fs::Permissions::from_mode(0o755)).unwrap();
        let inside_digest = sha256_hex(&fs::read(&inside).unwrap());
        assert_eq!(
            assess_file(&inside, &inside_digest),
            Err(HelperFault::Untrusted)
        );
        fs::set_permissions(&open, fs::Permissions::from_mode(0o1777)).unwrap();
        assert_eq!(assess_file(&inside, &inside_digest), Ok(()));
        assert!(blocks_pass(VERIFIED, true, false, "helper-missing"));
        assert!(blocks_pass(VERIFIED, true, false, "helper-mismatch"));
        assert!(blocks_pass(VERIFIED, true, false, "helper-untrusted"));
        assert!(blocks_pass(VERIFIED, true, false, "launch-failed"));
        assert!(blocks_pass(VERIFIED, true, false, "protocol-malformed"));
        assert!(!blocks_pass(VERIFIED, true, false, "ok"));
        assert!(!blocks_pass(VERIFIED, true, false, "stopped"));
        assert!(!blocks_pass(VERIFIED, true, false, "output-limit"));
        assert!(artifact_path().ends_with(
            "testing/harness/containment-supervisor/bin/containment-supervisor"
        ));
        #[cfg(target_os = "linux")]
        {
            let other = dir.join("other-uid");
            fs::write(&other, b"owned-by-other").unwrap();
            fs::set_permissions(&other, fs::Permissions::from_mode(0o755)).unwrap();
            let chown = ["/bin/chown", "/usr/bin/chown"]
                .into_iter()
                .find_map(|tool| {
                    Command::new(tool)
                        .args(["65534", &other.display().to_string()])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .ok()
                        .filter(|status| status.success())
                        .map(|_| ())
                });
            if chown.is_some() {
                let other_digest = sha256_hex(&fs::read(&other).unwrap());
                assert_eq!(
                    assess_file(&other, &other_digest),
                    Err(HelperFault::Untrusted)
                );
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn substituted_worker_is_not_executed() {
        let dir = private_dir("subst");
        let marker = dir.join("marker");
        let worker = dir.join("worker");
        fs::write(
            &worker,
            format!("#!/bin/sh\necho ran > {}\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&worker, fs::Permissions::from_mode(0o755)).unwrap();
        let digest = sha256_hex(&fs::read(&worker).unwrap());
        assert_eq!(
            assess_file(&worker, "abab"),
            Err(HelperFault::Mismatch)
        );
        assert!(!marker.exists(), "a rejected worker was executed");
        assert_ne!(digest, SUPERVISOR_SHA256);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn verified_complete_command_is_not_blocked() {
        let parsed = status_text(
            "containment=verified\nspawned=1\nexit_code=0\ncleanup_incomplete=0\noutput_limited=0\nnote=ok\ncgroup=1\n",
        )
        .unwrap();
        assert!(!blocks_pass(
            &parsed.containment,
            parsed.spawned,
            parsed.cleanup_incomplete,
            &parsed.note
        ));
        let stopped = status_text(
            "containment=verified\nspawned=1\nexit_code=-1\ncleanup_incomplete=0\noutput_limited=0\nnote=stopped\ncgroup=0\n",
        )
        .unwrap();
        assert!(!blocks_pass(
            &stopped.containment,
            stopped.spawned,
            stopped.cleanup_incomplete,
            &stopped.note
        ));
    }

    #[test]
    fn unavailable_unverified_and_uncertain_notes_cannot_pass() {
        for body in [
            "containment=unavailable\nspawned=0\nexit_code=-1\ncleanup_incomplete=1\noutput_limited=0\nnote=platform\ncgroup=0\n",
            "containment=unverified\nspawned=1\nexit_code=0\ncleanup_incomplete=1\noutput_limited=0\nnote=reap-timeout\ncgroup=0\n",
            "containment=verified\nspawned=1\nexit_code=0\ncleanup_incomplete=0\noutput_limited=0\nnote=identity-uncertain\ncgroup=0\n",
            "containment=verified\nspawned=1\nexit_code=0\ncleanup_incomplete=0\noutput_limited=0\nnote=signal-failed\ncgroup=0\n",
            "containment=verified\nspawned=1\nexit_code=0\ncleanup_incomplete=1\noutput_limited=0\nnote=ok\ncgroup=0\n",
            "containment=verified\nspawned=0\nexit_code=0\ncleanup_incomplete=0\noutput_limited=0\nnote=ok\ncgroup=0\n",
            "containment=unavailable\nspawned=0\nexit_code=-1\ncleanup_incomplete=1\noutput_limited=0\nnote=helper-missing\ncgroup=0\n",
            "containment=unavailable\nspawned=0\nexit_code=-1\ncleanup_incomplete=1\noutput_limited=0\nnote=protocol-malformed\ncgroup=0\n",
        ] {
            let parsed = status_text(body).unwrap();
            assert!(
                blocks_pass(
                    &parsed.containment,
                    parsed.spawned,
                    parsed.cleanup_incomplete,
                    &parsed.note
                ),
                "{body}"
            );
        }
    }

    #[test]
    fn partial_or_missing_status_is_not_a_pass() {
        assert!(status_text("containment=verified\n").is_none());
        assert!(status_text("not a status\n").is_none());
        assert!(blocks_pass(UNVERIFIED, false, true, "status-missing"));
        assert!(blocks_pass(UNAVAILABLE, false, true, "platform"));
    }
}
