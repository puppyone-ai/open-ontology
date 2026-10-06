//! Minimal Safe Rust process smoke. This is not the Pi adapter.
//! It runs on the pinned Linux arm64 Rust 1.94 image before that adapter exists.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    let mut failed: u32 = 0;
    failed += check("echo", check_echo);
    failed += check("exit_codes", check_exit_codes);
    failed += check("stdin_eof", check_stdin_eof);
    failed += check("partial_eof", check_partial_eof);
    failed += check("kill_wait", check_kill_wait);
    failed += check("deadline", check_deadline);
    if failed != 0 {
        eprintln!("process_smoke_failed={failed}");
        std::process::exit(1);
    }
    println!("process_smoke_ok");
}

fn check(name: &str, f: fn() -> Result<(), String>) -> u32 {
    match f() {
        Ok(()) => {
            println!("ok {name}");
            0
        }
        Err(err) => {
            eprintln!("fail {name}: {err}");
            1
        }
    }
}

fn check_echo() -> Result<(), String> {
    let out = Command::new("/bin/echo")
        .arg("pi-rpc-probe-smoke")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|err| format!("spawn echo: {err}"))?;
    if !out.status.success() {
        return Err(format!("echo status {}", out.status));
    }
    if out.stdout != b"pi-rpc-probe-smoke\n" {
        return Err(format!("echo stdout {:?}", out.stdout));
    }
    if !out.stderr.is_empty() {
        return Err(format!("echo stderr {:?}", out.stderr));
    }
    Ok(())
}

fn check_exit_codes() -> Result<(), String> {
    let ok = Command::new("/bin/true")
        .output()
        .map_err(|err| format!("spawn true: {err}"))?;
    if !ok.status.success() {
        return Err(format!("true status {}", ok.status));
    }
    let bad = Command::new("/bin/false")
        .output()
        .map_err(|err| format!("spawn false: {err}"))?;
    if bad.status.code() != Some(1) {
        return Err(format!("false status {}", bad.status));
    }
    Ok(())
}

fn check_stdin_eof() -> Result<(), String> {
    let mut child = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn cat: {err}"))?;
    {
        let stdin = child.stdin.as_mut().ok_or("cat stdin missing")?;
        stdin
            .write_all(b"line-one\n")
            .map_err(|err| format!("write cat: {err}"))?;
    }
    child.stdin.take();
    let mut stdout = Vec::new();
    child
        .stdout
        .as_mut()
        .ok_or("cat stdout missing")?
        .read_to_end(&mut stdout)
        .map_err(|err| format!("read cat: {err}"))?;
    let status = child.wait().map_err(|err| format!("wait cat: {err}"))?;
    if !status.success() {
        return Err(format!("cat status {status}"));
    }
    if stdout != b"line-one\n" {
        return Err(format!("cat stdout {:?}", stdout));
    }
    Ok(())
}

fn check_partial_eof() -> Result<(), String> {
    let mut child = Command::new("/usr/bin/printf")
        .arg("partial-no-lf")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn printf: {err}"))?;
    let mut stdout = Vec::new();
    child
        .stdout
        .as_mut()
        .ok_or("printf stdout missing")?
        .read_to_end(&mut stdout)
        .map_err(|err| format!("read printf: {err}"))?;
    let status = child.wait().map_err(|err| format!("wait printf: {err}"))?;
    if !status.success() {
        return Err(format!("printf status {status}"));
    }
    if stdout != b"partial-no-lf" {
        return Err(format!("printf stdout {:?}", stdout));
    }
    Ok(())
}

fn check_kill_wait() -> Result<(), String> {
    let mut child = Command::new("/bin/sleep")
        .arg("30")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn sleep: {err}"))?;
    child.kill().map_err(|err| format!("kill sleep: {err}"))?;
    let status = child.wait().map_err(|err| format!("wait sleep: {err}"))?;
    if status.success() {
        return Err(format!("killed sleep still success: {status}"));
    }
    Ok(())
}

fn check_deadline() -> Result<(), String> {
    let mut child = Command::new("/bin/sleep")
        .arg("30")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("spawn deadline sleep: {err}"))?;
    let started = Instant::now();
    let limit = Duration::from_millis(800);
    let mut finished = false;
    while started.elapsed() < limit {
        match child.try_wait() {
            Ok(Some(status)) => {
                finished = true;
                if status.success() {
                    return Err("deadline child exited 0 before the limit".to_owned());
                }
                break;
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(err) => return Err(format!("try_wait: {err}")),
        }
    }
    if !finished {
        child
            .kill()
            .map_err(|err| format!("deadline kill: {err}"))?;
        let status = child
            .wait()
            .map_err(|err| format!("deadline wait: {err}"))?;
        if status.success() {
            return Err(format!("deadline kill still success: {status}"));
        }
    }
    if started.elapsed() > Duration::from_secs(5) {
        return Err(format!("deadline elapsed {:?}", started.elapsed()));
    }
    println!("deadline_elapsed_ms={}", started.elapsed().as_millis());
    Ok(())
}
