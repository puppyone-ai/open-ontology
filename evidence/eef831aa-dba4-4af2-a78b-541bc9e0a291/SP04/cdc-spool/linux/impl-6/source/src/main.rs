//! JSONL stdin spool. No sockets, database, ACK, or source writes.

#![forbid(unsafe_code)]

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use cdc_spool::{
    recover, verify_replay, Checkpoint, Fault, FaultCode, Limits, Outcome, OutcomeKind, Recovery,
    Spool, CONTRACT,
};
use serde::Serialize;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        return usage("usage");
    };
    let Some(dir) = args.next() else {
        return usage("usage");
    };
    if args.next().is_some() {
        return usage("usage");
    }
    match command.as_str() {
        "spool" => cmd_spool(Path::new(&dir)),
        "recover" => cmd_recover(Path::new(&dir)),
        "replay" => cmd_replay(Path::new(&dir)),
        _ => usage("usage"),
    }
}

fn cmd_spool(dir: &Path) -> ExitCode {
    let mut spool = match Spool::create_new(dir, Limits::product()) {
        Ok(spool) => spool,
        Err(fault) => return fault_exit(&fault),
    };
    if install_checkpoint(&mut spool).is_err() {
        return usage("usage");
    }
    let stdin = io::stdin();
    match spool.ingest_reader(stdin.lock()) {
        Ok(outcome) => outcome_exit(&outcome),
        Err(fault) => fault_exit(&fault),
    }
}

fn cmd_recover(dir: &Path) -> ExitCode {
    match recover(dir) {
        Ok(Recovery::Committed(committed)) => {
            let line = CommittedLine {
                contract: CONTRACT,
                kind: "committed",
                txn_id: &committed.manifest.txn_id,
                count: committed.manifest.count,
                commit_lsn: &committed.manifest.commit_lsn,
                commit_position: &committed.manifest.commit_position,
                ack_allowed: false,
                target_applied: false,
                cross_table_consistent: false,
                applied: None,
            };
            if write_json(&line) {
                ExitCode::from(0)
            } else {
                ExitCode::from(1)
            }
        }
        Ok(Recovery::RolledBack(rolled)) => rolled_exit(rolled.txn_id.as_deref()),
        Ok(Recovery::Incomplete(pending)) => {
            let line = IncompleteLine {
                contract: CONTRACT,
                kind: "incomplete",
                durable_chunks: pending.durable_chunks,
                durable_events: pending.durable_events,
                ack_allowed: false,
                target_applied: false,
                cross_table_consistent: false,
                applied: None,
            };
            if write_json(&line) {
                ExitCode::from(3)
            } else {
                ExitCode::from(1)
            }
        }
        Ok(Recovery::Unknown { reason }) => {
            let line = UnknownLine {
                contract: CONTRACT,
                kind: "unknown",
                reason,
                ack_allowed: false,
                target_applied: false,
                cross_table_consistent: false,
                applied: None,
            };
            if write_json(&line) {
                ExitCode::from(3)
            } else {
                ExitCode::from(1)
            }
        }
        Err(fault) => fault_exit(&fault),
    }
}

fn cmd_replay(dir: &Path) -> ExitCode {
    let stdin = io::stdin();
    match verify_replay(dir, stdin.lock()) {
        Ok(()) => {
            let line = ReplayLine {
                contract: CONTRACT,
                kind: "replay_match",
                ack_allowed: false,
                target_applied: false,
                cross_table_consistent: false,
                applied: None,
            };
            if write_json(&line) {
                ExitCode::from(0)
            } else {
                ExitCode::from(1)
            }
        }
        Err(fault) => fault_exit(&fault),
    }
}

fn install_checkpoint(spool: &mut Spool) -> Result<(), ()> {
    let name = match std::env::var("CDC_SPOOL_CHECKPOINT") {
        Ok(name) => name,
        Err(std::env::VarError::NotPresent) => {
            if std::env::var_os("CDC_SPOOL_MARK").is_some() {
                return Err(());
            }
            return Ok(());
        }
        Err(_) => return Err(()),
    };
    let Ok(mark) = std::env::var("CDC_SPOOL_MARK") else {
        return Err(());
    };
    if mark.is_empty() {
        return Err(());
    }
    let want = match name.as_str() {
        "before_chunk_sync" => Checkpoint::BeforeChunkSync,
        "before_end" => Checkpoint::BeforeEnd,
        "before_final_seal" => Checkpoint::BeforeFinalSeal,
        _ => return Err(()),
    };
    spool.set_checkpoint(move |hit| {
        if hit == want {
            pause_at(Path::new(&mark))
        } else {
            Ok(())
        }
    });
    Ok(())
}

fn pause_at(mark: &Path) -> Result<(), Fault> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(mark)
        .map_err(|_| Fault::new(FaultCode::Io, None))?;
    file.write_all(b"ready\n")
        .map_err(|_| Fault::new(FaultCode::Io, None))?;
    file.sync_all()
        .map_err(|_| Fault::new(FaultCode::Io, None))?;
    let cont = PathBuf::from(format!("{}.continue", mark.display()));
    for _ in 0..600 {
        if cont.is_file() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(Fault::new(FaultCode::Io, None))
}

fn outcome_exit(outcome: &Outcome) -> ExitCode {
    match &outcome.kind {
        OutcomeKind::Sealed(summary) => {
            let line = SealedLine {
                contract: CONTRACT,
                kind: "sealed",
                txn_id: &summary.txn_id,
                count: summary.count,
                commit_lsn: &summary.commit_lsn,
                commit_position: &summary.commit_position,
                durable_events: summary.durable_events,
                decoded_events: summary.decoded_events,
                ack_allowed: false,
                target_applied: false,
                cross_table_consistent: false,
                applied: None,
                resident_hwm_bytes: summary.resident_hwm_bytes,
            };
            if write_json(&line) {
                ExitCode::from(0)
            } else {
                ExitCode::from(1)
            }
        }
        OutcomeKind::RolledBack { txn_id } => rolled_exit(Some(txn_id.as_str())),
        OutcomeKind::Incomplete { txn_id } => {
            let line = OpenLine {
                contract: CONTRACT,
                kind: "incomplete",
                txn_id: txn_id.as_deref(),
                durable_events: outcome.progress.durable_events,
                decoded_events: outcome.progress.decoded_events,
                ack_allowed: false,
                target_applied: false,
                cross_table_consistent: false,
                applied: None,
            };
            if write_json(&line) {
                ExitCode::from(3)
            } else {
                ExitCode::from(1)
            }
        }
    }
}

fn rolled_exit(txn_id: Option<&str>) -> ExitCode {
    let line = RolledLine {
        contract: CONTRACT,
        kind: "rolled_back",
        txn_id,
        ack_allowed: false,
        target_applied: false,
        cross_table_consistent: false,
        applied: None,
    };
    if write_json(&line) {
        ExitCode::from(0)
    } else {
        ExitCode::from(1)
    }
}

fn fault_exit(fault: &Fault) -> ExitCode {
    let line = RefusedLine {
        contract: CONTRACT,
        kind: "refused",
        fault: fault.code.to_string(),
        txn_id: fault.txn_id.as_deref(),
    };
    if !write_json(&line) {
        return ExitCode::from(1);
    }
    let _ = writeln!(io::stderr(), "cdc-spool: {}", fault.code);
    if fault.code == FaultCode::Io {
        ExitCode::from(1)
    } else {
        ExitCode::from(2)
    }
}

fn usage(reason: &str) -> ExitCode {
    let line = UsageLine {
        contract: CONTRACT,
        kind: "usage",
        reason,
    };
    if !write_json(&line) {
        return ExitCode::from(1);
    }
    let _ = writeln!(io::stderr(), "cdc-spool: {reason}");
    ExitCode::from(1)
}

fn write_json(value: &impl Serialize) -> bool {
    match serde_json::to_string(value) {
        Ok(body) => writeln!(io::stdout(), "{body}").is_ok(),
        Err(error) => {
            let _ = writeln!(io::stderr(), "cdc-spool: {error}");
            false
        }
    }
}

#[derive(Serialize)]
struct SealedLine<'a> {
    contract: &'a str,
    kind: &'a str,
    txn_id: &'a str,
    count: u64,
    commit_lsn: &'a str,
    commit_position: &'a str,
    durable_events: u64,
    decoded_events: u64,
    ack_allowed: bool,
    target_applied: bool,
    cross_table_consistent: bool,
    applied: Option<()>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resident_hwm_bytes: Option<u64>,
}

#[derive(Serialize)]
struct CommittedLine<'a> {
    contract: &'a str,
    kind: &'a str,
    txn_id: &'a str,
    count: u64,
    commit_lsn: &'a str,
    commit_position: &'a str,
    ack_allowed: bool,
    target_applied: bool,
    cross_table_consistent: bool,
    applied: Option<()>,
}

#[derive(Serialize)]
struct RolledLine<'a> {
    contract: &'a str,
    kind: &'a str,
    txn_id: Option<&'a str>,
    ack_allowed: bool,
    target_applied: bool,
    cross_table_consistent: bool,
    applied: Option<()>,
}

#[derive(Serialize)]
struct OpenLine<'a> {
    contract: &'a str,
    kind: &'a str,
    txn_id: Option<&'a str>,
    durable_events: u64,
    decoded_events: u64,
    ack_allowed: bool,
    target_applied: bool,
    cross_table_consistent: bool,
    applied: Option<()>,
}

#[derive(Serialize)]
struct IncompleteLine<'a> {
    contract: &'a str,
    kind: &'a str,
    durable_chunks: u64,
    durable_events: u64,
    ack_allowed: bool,
    target_applied: bool,
    cross_table_consistent: bool,
    applied: Option<()>,
}

#[derive(Serialize)]
struct UnknownLine<'a> {
    contract: &'a str,
    kind: &'a str,
    reason: &'a str,
    ack_allowed: bool,
    target_applied: bool,
    cross_table_consistent: bool,
    applied: Option<()>,
}

#[derive(Serialize)]
struct ReplayLine<'a> {
    contract: &'a str,
    kind: &'a str,
    ack_allowed: bool,
    target_applied: bool,
    cross_table_consistent: bool,
    applied: Option<()>,
}

#[derive(Serialize)]
struct RefusedLine<'a> {
    contract: &'a str,
    kind: &'a str,
    fault: String,
    txn_id: Option<&'a str>,
}

#[derive(Serialize)]
struct UsageLine<'a> {
    contract: &'a str,
    kind: &'a str,
    reason: &'a str,
}
