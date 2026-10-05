//! Pre-edit reproduction of the a195 grandchild-stdout hang.
//! This file is copied onto a private archive only after the smoke tests pass.
//! It is not a product test. It expects the unfixed supervisor to block.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use oo_gateway::parser_worker::{
    supervise_with, AdmitRequest, CancelFlag, SuperviseError, TrustedWorker,
};
use oo_gateway::{
    LiteralTypes, PolicyCatalog, PolicyIdentity, ResolvedFunction, ResolvedOperator,
    VisibleColumn, VisibleRelation,
};

const FAULT_SRC: &str = r#"
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn main() {
    match option_env!("OO_FAULT_MODE").unwrap_or("") {
        "orphan-stdout" => {
            let mut cmd = Command::new("/bin/sleep");
            cmd.arg("25").stdin(Stdio::null()).stderr(Stdio::null());
            cmd.stdout(Stdio::inherit());
            let child = cmd.spawn().expect("sleep");
            let id = child.id();
            std::mem::forget(child);
            let _ = std::fs::write("grandchild.pid", id.to_string());
            thread::sleep(Duration::from_secs(30));
        }
        _ => std::process::exit(2),
    }
}
"#;

fn note(line: &str) {
    let path = std::env::var("A201_ORACLE").unwrap_or_else(|_| "/tmp/a201-red-oracle.txt".to_string());
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|err| panic!("oracle {path}: {err}"));
    writeln!(file, "{line}").expect("oracle write");
    eprintln!("{line}");
}

fn catalog() -> PolicyCatalog {
    PolicyCatalog {
        identity: PolicyIdentity {
            project: "depot".to_string(),
            role: "oo_r_clerk".to_string(),
            policy_id: "policy-a201-red".to_string(),
        },
        relations: vec![VisibleRelation {
            schema: "depot".to_string(),
            name: "parcel".to_string(),
            physical_schema: "_oo".to_string(),
            physical_name: "pt_parcel".to_string(),
            columns: vec![VisibleColumn {
                name: "parcel_id".to_string(),
                physical_name: "pc_id".to_string(),
                type_oid: 23,
            }],
            pending_unclassified: 0,
            estimated_rows: 1,
        }],
        functions: vec![ResolvedFunction {
            oid: 8801,
            schema: "pg_catalog".to_string(),
            name: "count".to_string(),
            argument_type_oids: Vec::new(),
            aggregate_star: true,
        }],
        operators: vec![ResolvedOperator {
            oid: 8802,
            procedure_oid: 8802,
            schema: "pg_catalog".to_string(),
            name: "=".to_string(),
            left_type_oid: 23,
            right_type_oid: 23,
        }],
        literal_types: LiteralTypes {
            integer: Some(23),
            text: Some(25),
            boolean: Some(16),
        },
        null_test_allowed: true,
        large_table_row_threshold: 100,
    }
}

fn compile_fault() -> TrustedWorker {
    let dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("a201-red-fault");
    fs::create_dir_all(&dir).unwrap();
    let src = dir.join("fault.rs");
    fs::write(&src, FAULT_SRC).unwrap();
    let bin = dir.join("fault-orphan-stdout");
    let output = Command::new("rustc")
        .current_dir(&dir)
        .arg("--edition")
        .arg("2024")
        .arg("-D")
        .arg("warnings")
        .arg("-O")
        .arg("-o")
        .arg(&bin)
        .arg(&src)
        .env("OO_FAULT_MODE", "orphan-stdout")
        .output()
        .unwrap_or_else(|err| panic!("rustc: {err}"));
    assert!(
        output.status.success(),
        "rustc {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    TrustedWorker::approve(&bin).unwrap()
}

fn proc_field(pid: u32, index: usize) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let end = text.rfind(')')?;
    let from = end.checked_add(2)?;
    let rest = text.get(from..)?;
    rest.split_whitespace().nth(index)?.parse().ok()
}

#[test]
fn grandchild_stdout_blocks_finish_over_3s() {
    let worker = compile_fault();
    let req = AdmitRequest {
        identity: catalog().identity.clone(),
        catalog: catalog(),
        sql: "SELECT 1".to_string(),
    };
    let slot = Arc::new(Mutex::new((PathBuf::new(), 0u32, 0u32)));
    let observed = Arc::clone(&slot);
    let (tx, rx) = mpsc::channel();
    let started = Instant::now();
    thread::spawn(move || {
        let result = supervise_with(&worker, &req, &CancelFlag::new(), move |pid| {
            let cwd = fs::read_link(format!("/proc/{pid}/cwd")).unwrap_or_default();
            let look = Instant::now();
            let mut grandchild = 0u32;
            while look.elapsed() < Duration::from_millis(200) {
                if let Ok(text) = fs::read_to_string(cwd.join("grandchild.pid"))
                    && let Ok(parsed) = text.trim().parse::<u32>()
                {
                    grandchild = parsed;
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            *observed.lock().expect("slot") = (cwd, pid, grandchild);
        });
        let _ = tx.send(result);
    });
    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(result) => {
            let wall = started.elapsed();
            note(&format!(
                "case=grandchild-stdout-red hung=0 wall_ms={} result={result:?}",
                wall.as_millis()
            ));
            panic!("unfixed supervisor returned in {wall:?}; the a195 hang was not reproduced");
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let (cwd, child, grandchild) = slot.lock().expect("slot").clone();
            let alive = proc_field(grandchild, 19).is_some();
            let comm = fs::read_to_string(format!("/proc/{grandchild}/comm")).unwrap_or_default();
            note(&format!(
                "case=grandchild-stdout-red hung=1 at_ms=3000 child={child} grandchild={grandchild} alive={} comm={} cwd={}",
                u8::from(alive),
                comm.trim(),
                cwd.display()
            ));
            assert_ne!(grandchild, 0, "grandchild pid was not recorded");
            assert!(alive, "grandchild was already gone at 3s");
            assert!(comm.starts_with("sleep"), "comm {comm}");
            let _ = Command::new("/bin/kill")
                .arg("-KILL")
                .arg("--")
                .arg(grandchild.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(result) => {
                    let wall = started.elapsed();
                    let err = result.expect_err("hang returned Ok");
                    let SuperviseError::TimedOut(report) = err else {
                        panic!("unblocked result was not a timeout: {err:?}");
                    };
                    note(&format!(
                        "case=grandchild-stdout-red unblocked=1 wall_ms={} elapsed_ms={} signal={:?} reaped={} pid_absent={} direct={}",
                        wall.as_millis(),
                        report.elapsed.as_millis(),
                        report.term_signal,
                        u8::from(report.reaped),
                        u8::from(report.pid_absent),
                        report.pid
                    ));
                    assert!(wall > Duration::from_secs(3), "wall {wall:?}");
                    assert!(report.elapsed > Duration::from_secs(3), "{report:?}");
                    assert_eq!(report.term_signal, Some(9), "{report:?}");
                    assert!(report.reaped, "{report:?}");
                    assert_eq!(report.pid, child);
                }
                Err(_) => {
                    note("case=grandchild-stdout-red still_hung_after_grandchild_kill=1");
                    panic!("killing the grandchild did not unblock finish");
                }
            }
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("supervisor thread ended without a result");
        }
    }
}
