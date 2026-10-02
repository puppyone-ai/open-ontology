use std::io::{self, Write};

use crate::admit::admit_native;
use crate::decision::Decision;
use crate::policy::Policy;
use crate::procinfo::proc_starttime_self;
use crate::protocol::{read_frame, write_frame, KIND_ADMIT, KIND_READY, KIND_RESPONSE};

#[cfg(feature = "test-hooks")]
use crate::limits::PARSER_VERSION_NUM;
#[cfg(feature = "test-hooks")]
use crate::protocol::{KIND_CRASH, KIND_FAKE_ALLOW, KIND_GARBAGE, KIND_SPIN};
#[cfg(feature = "test-hooks")]
use std::thread;
#[cfg(feature = "test-hooks")]
use std::time::Duration;

/// One-shot parser worker. Stdout carries frames only.
pub fn run_worker() -> i32 {
    let mut args = std::env::args();
    let _argv0 = args.next();
    let Some(first) = args.next() else {
        eprintln!("worker_missing_nonce");
        return 2;
    };
    if first == "--supervise" {
        return crate::supervise::run(args);
    }
    if args.next().is_some() {
        eprintln!("worker_extra_args");
        return 2;
    }
    run_session(&first)
}

fn run_session(nonce: &str) -> i32 {
    let ready = serde_json::json!({
        "nonce": nonce,
        "pid": std::process::id(),
        "starttime": proc_starttime_self().unwrap_or_default(),
    });
    let mut stdout = io::stdout().lock();
    let body = match serde_json::to_vec(&ready) {
        Ok(body) => body,
        Err(_) => return 2,
    };
    if write_frame(&mut stdout, KIND_READY, &body).is_err() {
        return 2;
    }
    let frame = match read_frame(io::stdin().lock()) {
        Ok(frame) => frame,
        Err(code) => {
            eprintln!("worker_read={code}");
            return 2;
        }
    };
    match frame.kind {
        KIND_ADMIT => respond(&mut stdout, admit_body(&frame.body, nonce)),
        #[cfg(feature = "test-hooks")]
        KIND_SPIN => {
            loop {
                thread::sleep(Duration::from_secs(30));
            }
        }
        #[cfg(feature = "test-hooks")]
        KIND_CRASH => {
            eprintln!("worker_abort");
            std::process::abort();
        }
        #[cfg(feature = "test-hooks")]
        KIND_GARBAGE => {
            let _ = stdout.write_all(b"NOTAFRAME");
            let _ = stdout.flush();
            0
        }
        #[cfg(feature = "test-hooks")]
        KIND_FAKE_ALLOW => respond(&mut stdout, fake_allow(&frame.body, nonce)),
        _ => respond(&mut stdout, deny_stamped("INVALID_OUTPUT", nonce)),
    }
}

fn respond(stdout: &mut impl Write, decision: Decision) -> i32 {
    match write_decision(stdout, &decision) {
        Ok(()) => 0,
        Err(code) => {
            eprintln!("worker_write={code}");
            2
        }
    }
}

fn admit_body(body: &[u8], nonce: &str) -> Decision {
    let value: serde_json::Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(_) => return deny_stamped("INVALID_OUTPUT", nonce),
    };
    let Some(sql) = value.get("sql").and_then(|sql| sql.as_str()) else {
        return deny_stamped("INVALID_OUTPUT", nonce);
    };
    let policy = match value.get("policy") {
        Some(policy) => match serde_json::to_string(policy) {
            Ok(text) => match Policy::parse(&text) {
                Ok(policy) => policy,
                Err(_) => return deny_stamped("INVALID_OUTPUT", nonce),
            },
            Err(_) => return deny_stamped("INVALID_OUTPUT", nonce),
        },
        None => return deny_stamped("INVALID_OUTPUT", nonce),
    };
    stamp(admit_native(sql, &policy), nonce)
}

fn stamp(mut decision: Decision, nonce: &str) -> Decision {
    decision.nonce = nonce.to_string();
    decision.pid = std::process::id();
    decision.oid_acl_rls_proven = false;
    decision
}

fn deny_stamped(code: &str, nonce: &str) -> Decision {
    stamp(Decision::deny(code), nonce)
}

#[cfg(feature = "test-hooks")]
fn fake_allow(body: &[u8], nonce: &str) -> Decision {
    let mode = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("mode").and_then(|mode| mode.as_str()).map(str::to_string))
        .unwrap_or_else(|| "version".to_string());
    let mut decision = Decision::deny("ALLOW_SYNTAX");
    decision.syntax_allow = true;
    decision.code = "ALLOW_SYNTAX".to_string();
    decision.parser_version = if mode == "oid" { PARSER_VERSION_NUM } else { 1 };
    decision.oid_acl_rls_proven = mode == "oid";
    decision.nonce = nonce.to_string();
    decision.pid = std::process::id();
    decision
}

fn write_decision(stdout: &mut impl Write, decision: &Decision) -> Result<(), &'static str> {
    let body = serde_json::to_vec(decision).map_err(|_| "INVALID_OUTPUT")?;
    if body.len() > crate::limits::MAX_FRAME_BODY {
        let small = Decision::deny("RESPONSE_CAP");
        let body = serde_json::to_vec(&small).map_err(|_| "INVALID_OUTPUT")?;
        return write_frame(stdout, KIND_RESPONSE, &body);
    }
    write_frame(stdout, KIND_RESPONSE, &body)
}


