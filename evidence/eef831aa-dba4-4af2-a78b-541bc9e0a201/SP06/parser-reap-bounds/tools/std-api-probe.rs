//! Second look at Rust 1.94 std after the first probe failed to compile.
//! `Stdio` has no `From<UnixStream>`. This file uses the conversions the
//! compiler listed: `OwnedFd`, `PipeReader`, and `PipeWriter`.

use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn say(line: &str) {
    eprintln!("{line}");
    let _ = std::io::stderr().flush();
}

fn main() {
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(20));
        eprintln!("watchdog_exit=1");
        let _ = std::io::stderr().flush();
        std::process::exit(86);
    });
    say("probe=std-api-3");
    say("stdio_from_unixstream=unsupported_on_1_94");
    pipe_api();
    socket_via_owned_fd();
    read_timeout_kind();
    write_timeout_kind();
    kill_binary();
    direct_kill_leaves_grandchild();
    group_kill_forms();
    setsid_escape();
    say("probe_done=1");
}

fn pipe_api() {
    match io::pipe() {
        Ok((reader, writer)) => {
            say("std_io_pipe=ok");
            drop(reader);
            drop(writer);
        }
        Err(err) => say(&format!("std_io_pipe=err kind={:?}", err.kind())),
    }
}

fn socket_via_owned_fd() {
    say("step=owned_fd_begin");
    let (mut parent_read, child_write) = UnixStream::pair().expect("out");
    let (child_read, mut parent_write) = UnixStream::pair().expect("in");
    parent_read
        .set_read_timeout(Some(Duration::from_millis(300)))
        .expect("read timeout");
    parent_write
        .set_write_timeout(Some(Duration::from_millis(300)))
        .expect("write timeout");
    let child_out = OwnedFd::from(child_write);
    let child_in = OwnedFd::from(child_read);
    say("step=owned_fd_converted");
    let mut command = Command::new("/bin/cat");
    command
        .stdin(Stdio::from(child_in))
        .stdout(Stdio::from(child_out))
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = command.spawn().expect("cat");
    let pid = child.id();
    say(&format!("step=cat_spawned pid={pid}"));
    match parent_write.write_all(b"probe-bytes") {
        Ok(()) => say("step=cat_wrote"),
        Err(err) => say(&format!("step=cat_write_err kind={:?}", err.kind())),
    }
    drop(parent_write);
    let mut got = Vec::new();
    let mut buf = [0u8; 64];
    let deadline = Instant::now() + Duration::from_millis(400);
    loop {
        if Instant::now() >= deadline {
            say("step=cat_read_deadline");
            break;
        }
        match parent_read.read(&mut buf) {
            Ok(0) => {
                say("step=cat_eof");
                break;
            }
            Ok(n) => got.extend_from_slice(&buf[..n]),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => {
                say(&format!("step=cat_read_err kind={:?}", err.kind()));
                break;
            }
        }
    }
    let status = wait_bounded(&mut child, Duration::from_millis(400));
    say(&format!(
        "owned_fd_stdio pid={pid} status={status} bytes={}",
        String::from_utf8_lossy(&got)
    ));
    let _ = child.kill();
    let _ = wait_bounded(&mut child, Duration::from_millis(200));
}

fn wait_bounded(child: &mut std::process::Child, limit: Duration) -> String {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.to_string(),
            Ok(None) if started.elapsed() >= limit => return "still-running".to_string(),
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(err) => return format!("wait-err:{err}"),
        }
    }
}

fn read_timeout_kind() {
    let (mut reader, writer) = UnixStream::pair().expect("pair");
    match reader.set_read_timeout(Some(Duration::ZERO)) {
        Ok(()) => say("zero_read_timeout=accepted"),
        Err(err) => say(&format!("zero_read_timeout_kind={:?}", err.kind())),
    }
    reader
        .set_read_timeout(Some(Duration::from_millis(80)))
        .expect("set_read_timeout");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let started = Instant::now();
        let mut buf = [0u8; 1];
        let line = match reader.read(&mut buf) {
            Ok(n) => format!("read_timeout_unexpected_ok={n}"),
            Err(err) => format!(
                "read_timeout_kind={:?} elapsed_ms={}",
                err.kind(),
                started.elapsed().as_millis()
            ),
        };
        let _ = tx.send(line);
    });
    match rx.recv_timeout(Duration::from_millis(400)) {
        Ok(line) => say(&line),
        Err(_) => say("read_timeout_blocked=1 within_ms=400"),
    }
    drop(writer);
}

fn write_timeout_kind() {
    let (reader, mut writer) = UnixStream::pair().expect("pair");
    writer
        .set_write_timeout(Some(Duration::from_millis(80)))
        .expect("set_write_timeout");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let buf = vec![0u8; 64 * 1024];
        let started = Instant::now();
        let mut wrote = 0usize;
        let line = loop {
            match writer.write(&buf) {
                Ok(0) => break format!("write_timeout_kind=write0 wrote={wrote}"),
                Ok(n) => {
                    wrote += n;
                    if started.elapsed() > Duration::from_millis(400) {
                        break format!(
                            "write_timeout_kind=no_block_within_400ms wrote={wrote} elapsed_ms={}",
                            started.elapsed().as_millis()
                        );
                    }
                }
                Err(err) => {
                    break format!(
                        "write_timeout_kind={:?} wrote={wrote} elapsed_ms={}",
                        err.kind(),
                        started.elapsed().as_millis()
                    );
                }
            }
        };
        let _ = tx.send(line);
    });
    match rx.recv_timeout(Duration::from_millis(700)) {
        Ok(line) => say(&line),
        Err(_) => say("write_timeout_blocked=1 within_ms=700"),
    }
    drop(reader);
}

fn kill_binary() {
    for path in ["/bin/kill", "/usr/bin/kill"] {
        let meta = fs::symlink_metadata(path).ok();
        let kind = meta
            .as_ref()
            .map(|item| {
                if item.file_type().is_symlink() {
                    "symlink"
                } else if item.file_type().is_file() {
                    "file"
                } else {
                    "other"
                }
            })
            .unwrap_or("absent");
        let target = fs::read_link(path)
            .map(|item| item.display().to_string())
            .unwrap_or_default();
        say(&format!("kill_path={path} kind={kind} link_target={target}"));
    }
    match Command::new("/bin/kill").arg("--version").output() {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            let line = text.lines().next().unwrap_or("").trim();
            say(&format!(
                "kill_version_exit={} line={}",
                output.status.code().unwrap_or(-1),
                line
            ));
        }
        Err(err) => say(&format!("kill_version_err={err}")),
    }
    if let Ok(output) = Command::new("dpkg").args(["-S", "/bin/kill"]).output() {
        let text = String::from_utf8_lossy(&output.stdout);
        say(&format!(
            "dpkg_kill_exit={} line={}",
            output.status.code().unwrap_or(-1),
            text.lines().next().unwrap_or("").trim()
        ));
    }
}

fn spawn_sleep_group(tag: &str) -> (std::process::Child, u32, u32) {
    let mut child = Command::new("/bin/sh");
    child
        .arg("-c")
        .arg("sleep 40 & echo $!; wait")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = child
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {tag}: {err}"));
    let leader = child.id();
    let stdout = child.stdout.take().expect("stdout");
    let text = read_some(stdout, Duration::from_millis(500));
    let grand = text.trim().parse::<u32>().unwrap_or(0);
    say(&format!(
        "spawned tag={tag} leader={leader} grand={grand} line={} leader_pgrp={:?} grand_pgrp={:?} own_pgrp={:?}",
        text.trim(),
        pgrp(leader),
        pgrp(grand),
        pgrp(std::process::id())
    ));
    (child, leader, grand)
}

fn read_some(reader: impl Read + Send + 'static, limit: Duration) -> String {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 64];
        let n = reader.read(&mut buf).unwrap_or(0);
        let _ = tx.send(String::from_utf8_lossy(&buf[..n]).into_owned());
    });
    rx.recv_timeout(limit)
        .unwrap_or_else(|_| "read-timeout".to_string())
}

fn direct_kill_leaves_grandchild() {
    let (mut child, leader, grand) = spawn_sleep_group("direct");
    if grand == 0 {
        say("direct_kill skipped=no-grandchild");
        let _ = child.kill();
        let _ = wait_bounded(&mut child, Duration::from_millis(300));
        return;
    }
    let grand_start = start_ticks(grand);
    let _ = child.kill();
    let status = wait_bounded(&mut child, Duration::from_millis(300));
    thread::sleep(Duration::from_millis(50));
    say(&format!(
        "direct_kill status={status} leader_alive={} grand_alive={} grand_start={grand_start:?} grand_start_now={:?}",
        alive(leader),
        alive(grand),
        start_ticks(grand)
    ));
    let _ = signal_pid(grand);
    let _ = signal_pid(leader);
}

fn group_kill_forms() {
    let forms: &[&[&str]] = &[&["-KILL", "--"], &["-s", "KILL", "--"], &["-KILL"]];
    for form in forms {
        let (mut child, leader, grand) = spawn_sleep_group("group");
        let mut args: Vec<String> = form.iter().map(|item| (*item).to_string()).collect();
        args.push(format!("-{leader}"));
        let shown = args.join(",");
        let output = Command::new("/bin/kill")
            .args(&args)
            .stdin(Stdio::null())
            .output();
        thread::sleep(Duration::from_millis(80));
        let (code, err_line) = match &output {
            Ok(output) => (
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            ),
            Err(err) => (-1, err.to_string()),
        };
        say(&format!(
            "group_form args={shown} exit={code} stderr={err_line} leader_alive={} grand_alive={}",
            alive(leader),
            alive(grand)
        ));
        let _ = child.kill();
        let _ = wait_bounded(&mut child, Duration::from_millis(300));
        let _ = signal_pid(grand);
        let _ = signal_pid(leader);
    }
}

fn setsid_escape() {
    let setsid = if Path::new("/usr/bin/setsid").is_file() {
        "/usr/bin/setsid"
    } else if Path::new("/bin/setsid").is_file() {
        "/bin/setsid"
    } else {
        say("setsid_path=absent");
        return;
    };
    say(&format!("setsid_path={setsid}"));
    let script = format!("{setsid} /bin/sleep 40 & echo $!; wait");
    let mut child = Command::new("/bin/sh");
    child
        .arg("-c")
        .arg(&script)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = child.spawn().expect("setsid group");
    let leader = child.id();
    let stdout = child.stdout.take().expect("stdout");
    let text = read_some(stdout, Duration::from_millis(500));
    let recorded = text.trim().parse::<u32>().unwrap_or(0);
    thread::sleep(Duration::from_millis(100));
    say(&format!(
        "setsid_before leader={leader} leader_pgrp={:?} recorded={recorded} line={} recorded_pgrp={:?} recorded_comm={}",
        pgrp(leader),
        text.trim(),
        pgrp(recorded),
        comm(recorded)
    ));
    let _ = Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{leader}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    thread::sleep(Duration::from_millis(80));
    say(&format!(
        "setsid_after_group_kill leader_alive={} recorded_alive={} recorded_pgrp={:?} recorded_comm={}",
        alive(leader),
        alive(recorded),
        pgrp(recorded),
        comm(recorded)
    ));
    let _ = child.kill();
    let _ = wait_bounded(&mut child, Duration::from_millis(300));
    let _ = signal_pid(recorded);
    let _ = signal_pid(leader);
}

fn signal_pid(pid: u32) -> bool {
    if pid <= 1 {
        return false;
    }
    Command::new("/bin/kill")
        .args(["-KILL", "--", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn alive(pid: u32) -> u8 {
    u8::from(pid > 1 && Path::new(&format!("/proc/{pid}")).exists())
}

fn comm(pid: u32) -> String {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn pgrp(pid: u32) -> Option<u64> {
    stat_field(pid, 2)
}

fn start_ticks(pid: u32) -> Option<u64> {
    stat_field(pid, 19)
}

fn stat_field(pid: u32, index: usize) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let end = text.rfind(')')?;
    let from = end.checked_add(2)?;
    let rest = text.get(from..)?;
    rest.split_whitespace().nth(index)?.parse().ok()
}
