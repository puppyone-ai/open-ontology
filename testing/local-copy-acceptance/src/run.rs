//! Optional exec of a pinned producer binary.
//!
//! `run` never signs. An unfixed producer, a missing binary, or a pin mismatch
//! returns before `Command` is spawned. This module does not open a database socket.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::case::{Case, FACT_SINGLE};
use crate::fail::{fail, Fail};
use crate::sha::{hex_ok, sha256_hex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReport {
    pub executed: bool,
    pub exit_code: Option<i32>,
    pub reason: &'static str,
}

pub fn run_producer(binary: &Path, case: &Case) -> Result<RunReport, Fail> {
    if let Some(reason) = budget_block(case) {
        return Ok(stopped(reason));
    }
    if !case.producer.fixed_single_connection
        || case.producer.source_socket_fact != FACT_SINGLE
        || case.producer.source_socket_peak != Some(1)
    {
        return Ok(stopped("producer_unfixed"));
    }
    let meta = match fs::symlink_metadata(binary) {
        Ok(meta) => meta,
        Err(_) => return Ok(stopped("producer_absent")),
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Ok(stopped("producer_absent"));
    }
    let bytes = fs::read(binary).map_err(|_| fail("config", "producer binary"))?;
    let digest = sha256_hex(&bytes);
    if !hex_ok(&case.producer.binary_sha256) || digest != case.producer.binary_sha256 {
        return Ok(stopped("producer_pin_mismatch"));
    }
    if case.producer.argv.is_empty()
        || case
            .producer
            .argv
            .iter()
            .any(|arg| !Path::new(arg).is_absolute() || arg.contains('='))
    {
        return Ok(stopped("producer_argv"));
    }
    let mut child = Command::new(binary)
        .args(&case.producer.argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| fail("config", "producer spawn"))?;
    let limit = wait_limit(case);
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(RunReport {
                    executed: true,
                    exit_code: status.code(),
                    reason: "producer_started",
                });
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(RunReport {
                    executed: true,
                    exit_code: None,
                    reason: "producer_timeout",
                });
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => return Err(fail("config", "producer wait")),
        }
    }
}

fn stopped(reason: &'static str) -> RunReport {
    RunReport {
        executed: false,
        exit_code: None,
        reason,
    }
}

fn budget_block(case: &Case) -> Option<&'static str> {
    if !(1..=1000).contains(&case.budgets.batch_rows) {
        return Some("budget_batch");
    }
    if !(1..=30_000).contains(&case.budgets.statement_timeout_ms) {
        return Some("budget_statement");
    }
    if !(1..=1000).contains(&case.budgets.lock_timeout_ms) {
        return Some("budget_lock");
    }
    if !(1..=300_000).contains(&case.budgets.snapshot_deadline_ms) {
        return Some("budget_snapshot");
    }
    None
}

fn wait_limit(case: &Case) -> Duration {
    let snapshot = u64::try_from(case.budgets.snapshot_deadline_ms)
        .unwrap_or(300_000)
        .min(300_000);
    Duration::from_millis(snapshot.saturating_add(30_000))
}
