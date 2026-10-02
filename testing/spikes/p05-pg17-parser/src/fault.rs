//! Malformed workers for the explicit test-hooks feature.
//!
//! This binary is not a trusted parser. The parent runs it only from
//! `exercise_untrusted`, and only when the supervisor was built with
//! `test-hooks`. Production builds reject `allow-fault`.

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::thread;
use std::time::Duration;

use crate::decision::Decision;
use crate::limits::{PARSER_VERSION_NUM, PROTOCOL_VERSION};
use crate::procinfo::proc_starttime_self;
use crate::protocol::{write_frame, KIND_READY, KIND_RESPONSE, MAGIC};

pub fn run_fault_worker() -> i32 {
    // SAFETY: ignore SIGPIPE and SIGXFSZ in this fault process only, so a closed
    // pipe or the file-size cap returns EPIPE/EFBIG instead of killing the process
    // before it can emit a deny. This is not done in the production worker.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
    }
    let mut args = env::args();
    let _argv0 = args.next();
    let Some(nonce) = args.next() else {
        return 2;
    };
    let mode = env::var("OO_P05_FAULT").unwrap_or_default();
    match mode.as_str() {
        "premature" | "early-exit" => 0,
        "silent-sleep" => pause(),
        "bad-magic" => {
            let _ = io::stdout().write_all(b"XXXX");
            let _ = io::stdout().flush();
            pause()
        }
        "spoof-pid" => spoof_ready(&nonce, 1, &proc_starttime_self().unwrap_or_default()),
        "spoof-start" => spoof_ready(&nonce, std::process::id(), "1"),
        "spoof-nonce" => {
            let start = proc_starttime_self().unwrap_or_default();
            spoof_ready("deadbeef", std::process::id(), &start)
        }
        _ => {
            if write_ready(&nonce).is_err() {
                return 2;
            }
            match mode.as_str() {
                "partial" => {
                    let _ = io::stdout().write_all(b"P05");
                    let _ = io::stdout().flush();
                    pause()
                }
                "stdout-junk" => {
                    let junk = vec![b'A'; 1_000_000];
                    let _ = io::stdout().write_all(&junk);
                    let _ = io::stdout().flush();
                    pause()
                }
                "hold-setsid-noreply" => {
                    let _ = spawn_holder(true);
                    pause()
                }
                _ => after_request(&nonce, &mode),
            }
        }
    }
}

fn after_request(nonce: &str, mode: &str) -> i32 {
    if read_request().is_err() {
        return 2;
    }
    match mode {
        "oversize-frame" => oversize_frame(),
        "stderr-flood" => {
            let junk = vec![b'E'; 2_000_000];
            let _ = io::stderr().write_all(&junk);
            let _ = io::stderr().flush();
            let _ = write_decision(&shaped(nonce, false, "INVALID_OUTPUT", PARSER_VERSION_NUM, false, "", "stderr-flood"));
            0
        }
        "contradict" => {
            let _ = write_decision(&shaped(
                nonce,
                false,
                "ALLOW_SYNTAX",
                PARSER_VERSION_NUM,
                false,
                "DROP TABLE analytics.orders",
                "contradict",
            ));
            0
        }
        "oid-true" => {
            let _ = write_decision(&shaped(
                nonce,
                true,
                "ALLOW_SYNTAX",
                PARSER_VERSION_NUM,
                true,
                "SELECT secret_from_worker",
                "oid-true",
            ));
            0
        }
        "version-server" => {
            let _ = write_decision(&shaped(nonce, true, "ALLOW_SYNTAX", 170010, false, "SELECT 1", "version-server"));
            0
        }
        "extra-frame" => {
            let decision = shaped(nonce, false, "INVALID_OUTPUT", PARSER_VERSION_NUM, false, "", "extra");
            let _ = write_decision(&decision);
            let _ = write_decision(&decision);
            0
        }
        "hold-setsid-after" => {
            let _ = spawn_holder(true);
            let _ = write_decision(&shaped(nonce, false, "INVALID_OUTPUT", PARSER_VERSION_NUM, false, "", "hold-after"));
            pause()
        }
        "hold-samegroup" => {
            let _ = spawn_holder(false);
            let _ = write_decision(&shaped(nonce, false, "INVALID_OUTPUT", PARSER_VERSION_NUM, false, "", "hold-same"));
            pause()
        }
        "deny-with-sql" => {
            let _ = write_decision(&shaped(
                nonce,
                false,
                "NOT_SELECT",
                PARSER_VERSION_NUM,
                false,
                "DROP TABLE analytics.orders",
                "deny-with-sql",
            ));
            0
        }
        "unknown-field" => unknown_field(nonce),
        "unknown-code" => {
            let _ = write_decision(&shaped(nonce, false, "DROP_DATABASE", PARSER_VERSION_NUM, false, "", "unknown-code"));
            0
        }
        "oversize-sql" => {
            let sql = "S".repeat(70_000);
            let _ = write_decision(&shaped(nonce, true, "ALLOW_SYNTAX", PARSER_VERSION_NUM, false, &sql, "oversize-sql"));
            0
        }
        "consistent-allow" => {
            let _ = write_decision(&shaped(nonce, true, "ALLOW_SYNTAX", PARSER_VERSION_NUM, false, "SELECT 1", "consistent-allow"));
            0
        }
        "malformed-json" => {
            let _ = write_frame(&mut io::stdout(), KIND_RESPONSE, b"{");
            0
        }
        "boundary" => {
            let detail = boundary_detail();
            let _ = write_decision(&shaped(nonce, false, "INVALID_OUTPUT", PARSER_VERSION_NUM, false, "", &detail));
            0
        }
        "fsize" => fsize(nonce),
        _ => {
            let _ = write_decision(&shaped(nonce, false, "INVALID_OUTPUT", PARSER_VERSION_NUM, false, "", "unknown-mode"));
            0
        }
    }
}

fn pause() -> i32 {
    loop {
        thread::sleep(Duration::from_secs(30));
    }
}

fn write_ready(nonce: &str) -> io::Result<()> {
    let ready = serde_json::json!({
        "nonce": nonce,
        "pid": std::process::id(),
        "starttime": proc_starttime_self().unwrap_or_default(),
    });
    let body = serde_json::to_vec(&ready).map_err(|_| io::Error::other("json"))?;
    write_frame(&mut io::stdout(), KIND_READY, &body).map_err(|_| io::Error::other("frame"))?;
    Ok(())
}

fn spoof_ready(nonce: &str, pid: u32, starttime: &str) -> i32 {
    let ready = serde_json::json!({ "nonce": nonce, "pid": pid, "starttime": starttime });
    let Ok(body) = serde_json::to_vec(&ready) else {
        return 2;
    };
    let _ = write_frame(&mut io::stdout(), KIND_READY, &body);
    pause()
}

fn read_request() -> io::Result<()> {
    let mut stdin = io::stdin();
    let mut hdr = [0u8; 12];
    stdin.read_exact(&mut hdr)?;
    let len = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]) as usize;
    if len > crate::limits::MAX_FRAME_BODY {
        return Err(io::Error::other("cap"));
    }
    let mut body = vec![0u8; len];
    if len > 0 {
        stdin.read_exact(&mut body)?;
    }
    Ok(())
}

fn oversize_frame() -> i32 {
    let len = (crate::limits::MAX_FRAME_BODY as u32) + 1;
    let mut hdr = [0u8; 12];
    hdr[0..4].copy_from_slice(&MAGIC);
    hdr[4..6].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    hdr[6..8].copy_from_slice(&KIND_RESPONSE.to_le_bytes());
    hdr[8..12].copy_from_slice(&len.to_le_bytes());
    let _ = io::stdout().write_all(&hdr);
    let _ = io::stdout().write_all(&vec![b'B'; 70_000]);
    let _ = io::stdout().flush();
    pause()
}

fn shaped(nonce: &str, allow: bool, code: &str, version: i32, oid: bool, sql: &str, detail: &str) -> Decision {
    let mut decision = Decision::deny(code);
    decision.syntax_allow = allow;
    decision.code = code.to_string();
    decision.parser_version = version;
    decision.node_count = 1;
    decision.max_depth = 1;
    decision.executed_sql = sql.to_string();
    decision.detail = detail.to_string();
    decision.oid_acl_rls_proven = oid;
    decision.nonce = nonce.to_string();
    decision.pid = std::process::id();
    decision
}

fn write_decision(decision: &Decision) -> io::Result<()> {
    let body = serde_json::to_vec(decision).map_err(|_| io::Error::other("json"))?;
    write_frame(&mut io::stdout(), KIND_RESPONSE, &body).map_err(|_| io::Error::other("frame"))?;
    Ok(())
}

fn unknown_field(nonce: &str) -> i32 {
    let decision = shaped(nonce, false, "NOT_SELECT", PARSER_VERSION_NUM, false, "", "unknown-field");
    let Ok(mut value) = serde_json::to_value(&decision) else {
        return 2;
    };
    if let Some(map) = value.as_object_mut() {
        map.insert("injected".to_string(), serde_json::Value::Bool(true));
    }
    let Ok(body) = serde_json::to_vec(&value) else {
        return 2;
    };
    let _ = write_frame(&mut io::stdout(), KIND_RESPONSE, &body);
    0
}

fn fsize(nonce: &str) -> i32 {
    let mut wrote: u64 = 0;
    let buf = vec![0u8; 4096];
    if let Ok(mut file) = File::create("big.bin") {
        loop {
            match file.write(&buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    wrote = wrote.saturating_add(n as u64);
                    if wrote > 300_000 {
                        break;
                    }
                }
            }
        }
    }
    let detail = format!("wrote={wrote}");
    let _ = write_decision(&shaped(nonce, false, "RESOURCE_BUDGET", PARSER_VERSION_NUM, false, "", &detail));
    0
}

fn boundary_detail() -> String {
    let netns = fs::read_link("/proc/self/ns/net").map(|path| path.display().to_string()).unwrap_or_else(|_| "unreadable".to_string());
    let path = env::var("PATH").unwrap_or_default();
    let home = if env::var_os("HOME").is_some() { "set" } else { "unset" };
    let passwd = if File::open("/etc/passwd").is_ok() { "yes" } else { "no" };
    let cargo = if File::open("/src/Cargo.toml").is_ok() { "yes" } else { "no" };
    // SAFETY: socket() in this fault process probes whether the network
    // namespace can create a TCP socket. The fd is closed. Nothing is connected.
    let socket_fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    let socket = if socket_fd >= 0 {
        unsafe { libc::close(socket_fd) };
        "yes"
    } else {
        "no"
    };
    let nnp = nnp_bit();
    let mut fds = Vec::new();
    for fd in 0..64 {
        let Ok(target) = fs::read_link(format!("/proc/self/fd/{fd}")) else { continue };
        let text = target.to_string_lossy();
        if text.contains("/fd") {
            continue;
        }
        fds.push(fd.to_string());
    }
    format!(
        "netns={netns} path={path} home={home} passwd={passwd} cargo={cargo} socket={socket} nnp={nnp} fds={}",
        fds.join(",")
    )
}

fn nnp_bit() -> &'static str {
    let Ok(text) = fs::read_to_string("/proc/self/status") else {
        return "unreadable";
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("NoNewPrivs:") {
            return if rest.trim() == "1" { "1" } else { "0" };
        }
    }
    "absent"
}

fn spawn_holder(setsid_flag: bool) -> io::Result<()> {
    let mut fds = [0i32; 2];
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the fault worker is a single-threaded exec. The child uses only
    // async-signal-safe calls, then pauses. It does not run Rust destructors.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        return Err(io::Error::last_os_error());
    }
    if pid == 0 {
        unsafe {
            libc::close(fds[0]);
            if setsid_flag {
                libc::setsid();
            }
            let mark = [1u8];
            libc::write(fds[1], mark.as_ptr().cast(), 1);
            libc::close(fds[1]);
            loop {
                libc::pause();
            }
        }
    }
    unsafe { libc::close(fds[1]) };
    let mut mark = [0u8; 1];
    let _ = unsafe { libc::read(fds[0], mark.as_mut_ptr().cast(), 1) };
    unsafe { libc::close(fds[0]) };
    Ok(())
}
