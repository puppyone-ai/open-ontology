//! Behavioral checks for the real supervisor binary.
//!
//! These tests execute `CARGO_BIN_EXE_containment-supervisor`. They do not
//! accept a substitute worker. The debug binary and the pinned release binary
//! are the same program built from this crate; xtask pins the release file.

#[cfg(target_os = "linux")]
mod linux {
    use std::fs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    struct KillOnDrop(std::process::Child);

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    struct Outcome {
        code: i32,
        status: String,
        elapsed: Duration,
    }

    fn helper() -> PathBuf {
        PathBuf::from(env!("CARGO_BIN_EXE_containment-supervisor"))
    }

    fn work_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oo-a082-branch-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn starttime(pid: u32) -> Option<u64> {
        if pid <= 1 {
            return None;
        }
        let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let close = text.rfind(')')?;
        text[close + 1..]
            .split_whitespace()
            .nth(19)
            .and_then(|field| field.parse().ok())
    }

    fn field(status: &str, key: &str) -> String {
        status
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .unwrap_or("")
            .to_owned()
    }

    fn foreign_sleep() -> (KillOnDrop, u32, Option<u64>) {
        let child = Command::new("/bin/sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("foreign sleep");
        let pid = child.id();
        thread::sleep(Duration::from_millis(50));
        let start = starttime(pid);
        assert!(start.is_some(), "foreign sleep {pid} has no starttime");
        (KillOnDrop(child), pid, start)
    }

    fn assert_foreign(pid: u32, start: Option<u64>) {
        assert_eq!(starttime(pid), start, "foreign pid {pid} was reused or killed");
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        assert_eq!(comm.trim(), "sleep");
    }

    fn run(dir: &Path, limit: u64, args: &[&str], term_after: Option<Duration>) -> Outcome {
        let mut child = Command::new(helper())
            .arg(dir.join("status"))
            .arg(dir.join("stdout"))
            .arg(dir.join("stderr"))
            .arg(limit.to_string())
            .arg("--")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn supervisor");
        let started = Instant::now();
        if let Some(delay) = term_after {
            thread::sleep(delay);
            let killer = if std::path::Path::new("/bin/kill").is_file() {
                "/bin/kill"
            } else {
                "/usr/bin/kill"
            };
            let _ = Command::new(killer)
                .args(["-TERM", &child.id().to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let code = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.code().unwrap_or(-1),
                Ok(None) if started.elapsed() > Duration::from_secs(8) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break -9;
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(_) => break -1,
            }
        };
        let status = fs::read_to_string(dir.join("status")).unwrap_or_default();
        Outcome {
            code,
            status,
            elapsed: started.elapsed(),
        }
    }

    fn assert_settled(outcome: &Outcome) {
        assert!(
            outcome.elapsed < Duration::from_secs(8),
            "unbounded {:?}",
            outcome.elapsed
        );
        assert_eq!(outcome.code, 0, "{}", outcome.status);
        assert_eq!(field(&outcome.status, "containment"), "verified");
        assert_eq!(field(&outcome.status, "spawned"), "1");
        assert_eq!(field(&outcome.status, "cleanup_incomplete"), "0");
        assert!(field(&outcome.status, "cgroup") == "0" || field(&outcome.status, "cgroup") == "1");
    }

    fn gone(pid: u32, start: Option<u64>) {
        if pid <= 1 {
            return;
        }
        match (starttime(pid), start) {
            (None, _) => {}
            (Some(now), Some(then)) => {
                assert_ne!(now, then, "pid {pid} still has starttime {then}")
            }
            (Some(now), None) => panic!("pid {pid} is alive at starttime {now}"),
        }
    }

    fn setsid_script(wait_for_pid: bool) -> &'static str {
        if wait_for_pid {
            "#!/bin/sh\n\
pidfile=$1\n\
if [ -x /usr/bin/setsid ]; then SETSID=/usr/bin/setsid;\n\
elif [ -x /bin/setsid ]; then SETSID=/bin/setsid;\n\
else echo NO_SETSID > \"$pidfile\"; exit 9; fi\n\
\"$SETSID\" /bin/sh -c 'echo $$ > \"$1\"; cat /proc/$$/stat >> \"$1\"; exec /bin/sleep 121' _ \"$pidfile\" >/dev/null 2>&1 &\n\
i=0\n\
while [ ! -s \"$pidfile\" ] && [ \"$i\" -lt 200 ]; do i=$((i+1)); sleep 0.01; done\n\
exit 0\n"
        } else {
            "#!/bin/sh\n\
pidfile=$1\n\
if [ -x /usr/bin/setsid ]; then SETSID=/usr/bin/setsid;\n\
elif [ -x /bin/setsid ]; then SETSID=/bin/setsid;\n\
else echo NO_SETSID; exit 9; fi\n\
\"$SETSID\" /bin/sh -c 'echo $$ > \"$1\"; cat /proc/$$/stat >> \"$1\"; exec /bin/sleep 121' _ \"$pidfile\" >/dev/null 2>&1 &\n\
exit 0\n"
        }
    }

    fn pid_and_start(text: &str) -> (u32, Option<u64>) {
        let mut lines = text.lines();
        let pid = lines.next().unwrap_or("").trim().parse().unwrap_or(0);
        let stat = lines.next().unwrap_or("");
        let start = stat.rfind(')').and_then(|close| {
            stat[close + 1..]
                .split_whitespace()
                .nth(19)
                .and_then(|field| field.parse().ok())
        });
        (pid, start)
    }

    #[test]
    fn exact_output_seals_and_one_extra_byte_stops_the_file() {
        let (_foreign, foreign_pid, foreign_start) = foreign_sleep();
        let dir = work_dir("exact");
        let outcome = run(&dir, 6, &["/bin/echo", "hello"], None);
        assert_settled(&outcome);
        assert_eq!(field(&outcome.status, "output_limited"), "0");
        assert_eq!(field(&outcome.status, "exit_code"), "0");
        assert_eq!(field(&outcome.status, "note"), "ok");
        assert_eq!(fs::read(dir.join("stdout")).unwrap(), b"hello\n");
        let over = work_dir("over");
        let limited = run(&over, 5, &["/bin/sh", "-c", "printf hello; printf x"], None);
        assert_settled(&limited);
        assert_eq!(field(&limited.status, "output_limited"), "1");
        assert_eq!(field(&limited.status, "note"), "output-limit");
        // The helper keeps the payload's real status. xtask turns a limited
        // zero exit into -1. A fast printf can exit 0 before the kill lands.
        let len = fs::metadata(over.join("stdout")).unwrap().len();
        thread::sleep(Duration::from_millis(200));
        let after = fs::metadata(over.join("stdout")).unwrap();
        assert_eq!(len, 5);
        assert_eq!(after.len(), len);
        assert_eq!(after.ino(), fs::metadata(over.join("stdout")).unwrap().ino());
        assert_foreign(foreign_pid, foreign_start);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&over);
    }

    #[test]
    fn protocol_malformed_does_not_spawn() {
        let (_foreign, foreign_pid, foreign_start) = foreign_sleep();
        let dir = work_dir("bad");
        let started = Instant::now();
        let output = Command::new(helper())
            .arg(dir.join("status"))
            .arg(dir.join("stdout"))
            .arg(dir.join("stderr"))
            .arg("0")
            .arg("--")
            .arg("/bin/sleep")
            .arg("30")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(output.status.code(), Some(82));
        let status = fs::read_to_string(dir.join("status")).unwrap();
        assert_eq!(field(&status, "containment"), "unavailable");
        assert_eq!(field(&status, "spawned"), "0");
        assert_eq!(field(&status, "note"), "protocol-malformed");
        assert_eq!(field(&status, "cleanup_incomplete"), "1");
        assert!(fs::read(dir.join("stdout")).unwrap_or_default().is_empty());
        assert_foreign(foreign_pid, foreign_start);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn timeout_reaps_the_payload_and_returns_bounded() {
        let (_foreign, foreign_pid, foreign_start) = foreign_sleep();
        let dir = work_dir("term");
        let outcome = run(&dir, 1024, &["/bin/sleep", "30"], Some(Duration::from_millis(200)));
        assert_settled(&outcome);
        assert_eq!(field(&outcome.status, "note"), "stopped");
        let direct: u32 = field(&outcome.status, "direct_pid").parse().unwrap_or(0);
        assert!(
            starttime(direct).is_none(),
            "direct pid {direct} still has a starttime"
        );
        assert!(outcome.elapsed < Duration::from_secs(4), "{:?}", outcome.elapsed);
        assert_foreign(foreign_pid, foreign_start);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recorded_setsid_double_fork_is_gone() {
        let (_foreign, foreign_pid, foreign_start) = foreign_sleep();
        let dir = work_dir("setsid");
        let script = dir.join("escape.sh");
        let pidfile = dir.join("grandchild.pid");
        fs::write(&script, setsid_script(true)).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let outcome = run(
            &dir,
            1024,
            &["/bin/sh", script.to_str().unwrap(), pidfile.to_str().unwrap()],
            None,
        );
        let recorded = fs::read_to_string(&pidfile).unwrap_or_default();
        assert!(!recorded.contains("NO_SETSID"), "{recorded}");
        let (pid, start) = pid_and_start(&recorded);
        assert_settled(&outcome);
        assert!(pid > 1, "{recorded}");
        assert!(start.is_some(), "{recorded}");
        gone(pid, start);
        assert_foreign(foreign_pid, foreign_start);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn immediate_setsid_double_fork_is_gone() {
        let (_foreign, foreign_pid, foreign_start) = foreign_sleep();
        let dir = work_dir("immediate");
        let script = dir.join("escape.sh");
        let pidfile = dir.join("grandchild.pid");
        fs::write(&script, setsid_script(false)).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let outcome = run(
            &dir,
            1024,
            &["/bin/sh", script.to_str().unwrap(), pidfile.to_str().unwrap()],
            None,
        );
        assert_settled(&outcome);
        let recorded = fs::read_to_string(&pidfile).unwrap_or_default();
        assert!(!recorded.contains("NO_SETSID"), "{recorded}");
        let (pid, start) = pid_and_start(&recorded);
        assert!(pid > 1, "{recorded}");
        assert!(start.is_some(), "{recorded}");
        gone(pid, start);
        assert_foreign(foreign_pid, foreign_start);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fork_chain_is_gone() {
        let (_foreign, foreign_pid, foreign_start) = foreign_sleep();
        let dir = work_dir("chain");
        let script = dir.join("chain.sh");
        let pidfile = dir.join("leaf.pid");
        fs::write(
            &script,
            "#!/bin/sh\n\
pidfile=$1\n\
depth=$2\n\
if [ \"$depth\" -eq 0 ]; then\n\
  /bin/sleep 121 >/dev/null 2>&1 &\n\
  echo $! > \"$pidfile\"\n\
  cat /proc/$!/stat >> \"$pidfile\"\n\
  exit 0\n\
fi\n\
/bin/sh \"$0\" \"$pidfile\" $((depth - 1)) >/dev/null 2>&1 &\n\
if [ \"$depth\" -eq 3 ]; then\n\
  i=0\n\
  while [ ! -s \"$pidfile\" ] && [ \"$i\" -lt 200 ]; do i=$((i+1)); sleep 0.01; done\n\
fi\n\
exit 0\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let outcome = run(
            &dir,
            1024,
            &[
                "/bin/sh",
                script.to_str().unwrap(),
                pidfile.to_str().unwrap(),
                "3",
            ],
            None,
        );
        assert_settled(&outcome);
        let recorded = fs::read_to_string(&pidfile).unwrap_or_default();
        let (pid, start) = pid_and_start(&recorded);
        assert!(pid > 1 && start.is_some(), "{recorded}");
        gone(pid, start);
        assert_foreign(foreign_pid, foreign_start);
        let _ = fs::remove_dir_all(&dir);
    }

    fn hold_script(stderr: bool) -> String {
        let write = if stderr {
            "printf x >> \"$1\"; printf y >&2;"
        } else {
            "printf x >> \"$1\"; printf y;"
        };
        format!(
            "#!/bin/sh\n\
side=$1\npidfile=$2\n\
if [ -x /usr/bin/setsid ]; then SETSID=/usr/bin/setsid;\n\
elif [ -x /bin/setsid ]; then SETSID=/bin/setsid;\n\
else echo NO_SETSID > \"$pidfile\"; exit 9; fi\n\
\"$SETSID\" /bin/sh -c 'printf x > \"$1\"; echo $$ > \"$2\"; cat /proc/$$/stat >> \"$2\"; while true; do {write} done' _ \"$side\" \"$pidfile\" >/dev/null 2>&1 &\n\
i=0\n\
while [ ! -s \"$pidfile\" ] && [ \"$i\" -lt 200 ]; do i=$((i+1)); sleep 0.01; done\n\
exit 0\n",
            write = write
        )
    }

    fn held_stream_is_stable(label: &str, stderr: bool) {
        let (_foreign, foreign_pid, foreign_start) = foreign_sleep();
        let dir = work_dir(label);
        let script = dir.join("hold.sh");
        let side = dir.join("side.bin");
        let pidfile = dir.join("writer.pid");
        fs::write(&script, hold_script(stderr)).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let outcome = run(
            &dir,
            4096,
            &[
                "/bin/sh",
                script.to_str().unwrap(),
                side.to_str().unwrap(),
                pidfile.to_str().unwrap(),
            ],
            None,
        );
        assert_settled(&outcome);
        let recorded = fs::read_to_string(&pidfile).unwrap_or_default();
        assert!(!recorded.contains("NO_SETSID"), "{recorded}");
        let (pid, start) = pid_and_start(&recorded);
        assert!(pid > 1 && start.is_some(), "{recorded}");
        gone(pid, start);
        let capture = if stderr { "stderr" } else { "stdout" };
        let meta = fs::metadata(dir.join(capture)).unwrap();
        let side_meta = fs::metadata(&side).ok();
        thread::sleep(Duration::from_millis(300));
        let meta_after = fs::metadata(dir.join(capture)).unwrap();
        let side_after = fs::metadata(&side).ok();
        assert_eq!(meta.len(), meta_after.len());
        assert_eq!(meta.ino(), meta_after.ino());
        assert!(
            side_meta.as_ref().is_some_and(|item| item.len() > 0),
            "side file was not written"
        );
        assert_eq!(
            side_meta.as_ref().map(|item| item.len()),
            side_after.as_ref().map(|item| item.len())
        );
        assert_eq!(
            side_meta.as_ref().map(|item| item.ino()),
            side_after.as_ref().map(|item| item.ino())
        );
        assert!(meta.len() <= 4096);
        assert_foreign(foreign_pid, foreign_start);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn held_stdout_does_not_grow_after_return() {
        held_stream_is_stable("hold-out", false);
    }

    #[test]
    fn held_stderr_does_not_grow_after_return() {
        held_stream_is_stable("hold-err", true);
    }
}

#[cfg(not(target_os = "linux"))]
#[test]
fn macos_helper_reports_platform_and_does_not_spawn() {
    use std::fs;
    use std::process::{Command, Stdio};
    let dir = std::env::temp_dir().join(format!("oo-a082-platform-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_containment-supervisor"))
        .arg(dir.join("status"))
        .arg(dir.join("stdout"))
        .arg(dir.join("stderr"))
        .arg("32")
        .arg("--")
        .arg("/bin/echo")
        .arg("nope")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(81));
    let status = fs::read_to_string(dir.join("status")).unwrap();
    assert!(status.contains("containment=unavailable\n"));
    assert!(status.contains("spawned=0\n"));
    assert!(status.contains("note=platform\n"));
    assert!(!dir.join("stdout").exists());
    let _ = fs::remove_dir_all(&dir);
}
