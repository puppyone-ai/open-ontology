//! Local spool inspect and GC. This process does not listen.

use std::env;
use std::path::Path;
use std::process::ExitCode;

use gateway_audit_wal::{inspect, DiskWal, WalConfig};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let command = args.next();
    let flag = args.next();
    let path = args.next();
    if args.next().is_some() {
        eprintln!("usage");
        return ExitCode::from(2);
    }
    match (command.as_deref(), flag.as_deref(), path.as_deref()) {
        (Some("replay"), Some("--spool"), Some(path)) => replay(Path::new(path)),
        (Some("gc"), Some("--spool"), Some(path)) => gc(Path::new(path)),
        _ => {
            eprintln!("usage");
            ExitCode::from(2)
        }
    }
}

fn replay(path: &Path) -> ExitCode {
    let view = match inspect(path) {
        Ok(view) => view,
        Err(err) => {
            eprintln!("replay={}", err.name());
            return ExitCode::from(1);
        }
    };
    println!("spool={}", view.spool_id);
    println!("torn={}", view.torn_tail);
    for frame in &view.frames {
        println!(
            "frame request={} seq={} kind={} project={} role={} hash={} segment={}",
            frame.request_id, frame.event_seq, frame.kind, frame.project, frame.role, frame.payload_hash, frame.segment_id
        );
    }
    for request in &view.requests {
        println!(
            "request id={} state={} project={} role={}",
            request.request_id, request.state, request.project, request.role
        );
    }
    ExitCode::SUCCESS
}

fn gc(path: &Path) -> ExitCode {
    let mut wal = match DiskWal::open(path, WalConfig::default()) {
        Ok(wal) => wal,
        Err(err) => {
            eprintln!("gc={}", err.name());
            return ExitCode::from(1);
        }
    };
    match wal.gc_acknowledged() {
        Ok(removed) => {
            println!("removed={removed}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("gc={}", err.name());
            ExitCode::from(1)
        }
    }
}
