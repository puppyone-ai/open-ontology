use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use oo_p05_pg17_parser::{admit_in_worker, admit_native, exercise_worker, Decision, Policy};

fn main() {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "dump".to_string());
    match cmd.as_str() {
        "dump" => dump(args.next().unwrap_or_else(|| "/out/worker-plain".to_string())),
        "ipc" => ipc(args.next().expect("worker")),
        "paths" => paths(),
        "inherit" => inherit(args.next().expect("worker")),
        other => {
            eprintln!("unknown {other}");
            std::process::exit(2);
        }
    }
}

fn dump(worker: String) {
    let policy = Policy::parse(&fs::read_to_string("/src/fixtures/policy.json").unwrap()).unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(&fs::read_to_string("/probe/cases.json").unwrap()).unwrap();
    let worker = PathBuf::from(worker);
    let mut out = fs::File::create("/out/decisions.jsonl").unwrap();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let sql = case["sql"].as_str().unwrap();
        let native = admit_native(sql, &policy);
        let remote = admit_in_worker(&worker, sql, &policy);
        let row = serde_json::json!({
            "id": id,
            "native": snap(&native),
            "remote": snap(&remote),
            "remote_reaped": remote.reaped,
            "remote_workdir_removed": remote.workdir_removed,
            "remote_starttime_matched": remote.starttime_matched,
            "remote_pid": remote.pid,
            "same_sql": native.executed_sql == remote.executed_sql,
            "same_code": native.code == remote.code,
        });
        writeln!(out, "{row}").unwrap();
        println!("{id} native={} remote={} reaped={}", native.code, remote.code, remote.reaped);
    }
}

fn snap(decision: &Decision) -> serde_json::Value {
    serde_json::json!({
        "syntax_allow": decision.syntax_allow,
        "code": decision.code,
        "parser_version": decision.parser_version,
        "node_count": decision.node_count,
        "max_depth": decision.max_depth,
        "original_projection_star": decision.original_projection_star,
        "executed_sql": decision.executed_sql,
        "detail": decision.detail,
        "oid_acl_rls_proven": decision.oid_acl_rls_proven,
        "pid": decision.pid,
        "reaped": decision.reaped,
        "workdir_removed": decision.workdir_removed,
        "starttime_matched": decision.starttime_matched,
    })
}

fn ipc(worker: String) {
    let started = Instant::now();
    println!("probe_enter");
    let _ = io::stdout().flush();
    let decision = exercise_worker(std::path::Path::new(&worker), 2, br#"{"sql":"SELECT 1","policy":{"relations":{}}}"#);
    let elapsed = started.elapsed().as_millis();
    let mut line = serde_json::to_string(&snap(&decision)).unwrap();
    let alive = process_alive(decision.pid);
    let leftovers: Vec<_> = fs::read_dir("/tmp")
        .map(|dir| {
            dir.filter_map(|item| item.ok())
                .map(|item| item.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("oo-p05-"))
                .collect()
        })
        .unwrap_or_default();
    println!(
        "elapsed_ms={elapsed} alive_after={} leftovers={} decision={line}",
        alive,
        leftovers.join(",")
    );
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as i32, 0) };
    rc == 0
}

fn paths() {
    let policy = Policy::parse(&fs::read_to_string("/src/fixtures/policy.json").unwrap()).unwrap();
    let sql = "SELECT 1";
    for (label, path) in [
        ("missing", "/probe/no-such-worker"),
        ("directory", "/probe"),
        ("symlink", "/probe/bin/worker-link"),
        ("relative", "bin/quick-deny"),
    ] {
        let started = Instant::now();
        let decision = if label == "relative" {
            let _ = std::env::set_current_dir("/probe");
            admit_in_worker(std::path::Path::new(path), sql, &policy)
        } else {
            admit_in_worker(std::path::Path::new(path), sql, &policy)
        };
        println!(
            "{label} elapsed_ms={} code={} allow={} oid={} sql_empty={} reaped={} workdir={} pid={}",
            started.elapsed().as_millis(),
            decision.code,
            decision.syntax_allow,
            decision.oid_acl_rls_proven,
            decision.executed_sql.is_empty(),
            decision.reaped,
            decision.workdir_removed,
            decision.pid
        );
    }
}

fn inherit(worker: String) {
    let mut marker = fs::File::create("/probe/secret-marker.txt").unwrap();
    writeln!(marker, "a076-secret-marker-not-a-credential").unwrap();
    // Keep the descriptor open across spawn. Leak is intentional and process-local.
    let _held = Box::leak(Box::new(marker));
    ipc(worker);
}
