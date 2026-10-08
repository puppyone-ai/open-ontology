//! Exact argv, cleared environment, bounded child, kill then wait.

use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use zeroize::Zeroize;

#[derive(Clone, Debug)]
pub struct ChildSpec {
    pub binary: String,
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
    pub deadline: Duration,
    pub needles: Vec<Vec<u8>>,
    pub cancel: Arc<AtomicBool>,
    pub pid_out: Arc<AtomicU32>,
}

#[derive(Debug)]
pub struct ChildOutput {
    pub exit: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub killed: bool,
    pub child_exited: bool,
    pub secret_leak: bool,
    pub pid: Option<u32>,
}

pub async fn run_child(spec: ChildSpec) -> ChildOutput {
    match tokio::task::spawn_blocking(move || run_blocking(spec)).await {
        Ok(output) => output,
        Err(_) => failed(),
    }
}

fn run_blocking(spec: ChildSpec) -> ChildOutput {
    if !Path::new(&spec.binary).is_absolute() {
        return failed();
    }
    let mut command = Command::new(&spec.binary);
    command
        .args(&spec.args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return failed(),
    };
    let pid = child.id();
    spec.pid_out.store(pid, Ordering::SeqCst);
    if let Some(mut stdin) = child.stdin.take()
        && stdin.write_all(&spec.stdin).is_err()
    {
        let _ = child.kill();
        let child_exited = child.wait().is_ok();
        return ChildOutput {
            exit: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            killed: true,
            child_exited,
            secret_leak: false,
            pid: Some(pid),
        };
    }
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_thread = thread::spawn(move || read_pipe(stdout));
    let err_thread = thread::spawn(move || read_pipe(stderr));
    let started = Instant::now();
    let mut exit = None;
    let mut killed = false;
    let mut child_exited = false;
    loop {
        if spec.cancel.load(Ordering::SeqCst) || started.elapsed() > spec.deadline {
            let _ = child.kill();
            killed = true;
            child_exited = child.wait().is_ok();
            break;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                exit = status.code();
                child_exited = true;
                break;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }
    let mut out = out_thread.join().unwrap_or_default();
    let mut err = err_thread.join().unwrap_or_default();
    let secret_leak = spec
        .needles
        .iter()
        .any(|needle| !needle.is_empty() && (contains(&out, needle) || contains(&err, needle)));
    if secret_leak {
        out.clear();
        err.clear();
    }
    ChildOutput {
        exit,
        stdout: out,
        stderr: err,
        killed,
        child_exited,
        secret_leak,
        pid: Some(pid),
    }
}

fn read_pipe(pipe: Option<impl Read>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut buf);
    }
    buf
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

fn failed() -> ChildOutput {
    ChildOutput {
        exit: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
        killed: false,
        child_exited: false,
        secret_leak: false,
        pid: None,
    }
}

pub fn may_mark_canceled(child_exited: bool) -> bool {
    child_exited
}

impl Drop for ChildSpec {
    fn drop(&mut self) {
        self.stdin.zeroize();
        for needle in &mut self.needles {
            needle.zeroize();
        }
    }
}
