#![forbid(unsafe_code)]

mod common;

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use cdc_spool::{recover, Checkpoint, Fault, FaultCode, Limits, Recovery, Spool};
use common::{begin, bind, data, end, feed, tempdir, Row, TS_Z};
use serde_json::json;

#[test]
fn child_checkpoint_hold() {
    if std::env::var_os("CDC_SPOOL_CHILD").is_none() {
        return;
    }
    let dir = std::env::var("CDC_SPOOL_DIR").expect("dir");
    let mark = std::env::var("CDC_SPOOL_MARK").expect("mark");
    let feed_path = std::env::var("CDC_SPOOL_FEED").expect("feed");
    let which = std::env::var("CDC_SPOOL_CHECKPOINT").expect("checkpoint");
    let want = match which.as_str() {
        "before_chunk_sync" => Checkpoint::BeforeChunkSync,
        "before_end" => Checkpoint::BeforeEnd,
        "before_final_seal" => Checkpoint::BeforeFinalSeal,
        other => panic!("unknown checkpoint {other}"),
    };
    let mut limits = Limits::product();
    limits.max_chunk_events = 1;
    let mut spool = match Spool::create_new(Path::new(&dir), limits) {
        Ok(spool) => spool,
        Err(err) => {
            eprintln!("create {err}");
            std::process::exit(2);
        }
    };
    spool.set_checkpoint(move |hit| {
        if hit != want {
            return Ok(());
        }
        pause(Path::new(&mark))
    });
    let file = match File::open(feed_path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("feed {err}");
            std::process::exit(2);
        }
    };
    if let Err(err) = spool.ingest_reader(BufReader::new(file)) {
        eprintln!("ingest {err}");
        std::process::exit(2);
    }
}

fn pause(mark: &Path) -> Result<(), Fault> {
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

fn kill_feed() -> Vec<u8> {
    feed(&[
        bind(&["public.lines", "public.orders"]),
        begin("kill1"),
        data(&Row {
            txn: "kill1",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "1",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
        data(&Row {
            txn: "kill1",
            global: 2,
            collection: 1,
            table: "public.lines",
            key: "1",
            op: "delete",
            payload: json!([]),
            unavailable: &[],
        }),
        end(
            "kill1",
            2,
            &[("public.lines", 1), ("public.orders", 1)],
            "0/2",
            TS_Z,
        ),
    ])
}

struct Child {
    root: PathBuf,
    dir: PathBuf,
    child: std::process::Child,
}

fn spawn(checkpoint: &str) -> Child {
    let root = tempdir();
    let dir = root.join("txn");
    let mark = root.join("mark");
    let feed_path = root.join("feed.jsonl");
    fs::write(&feed_path, kill_feed()).unwrap();
    let stdout = File::create(root.join("child.out")).unwrap();
    let stderr = File::create(root.join("child.err")).unwrap();
    let child = Command::new(std::env::current_exe().expect("exe"))
        .arg("child_checkpoint_hold")
        .arg("--exact")
        .arg("--test-threads=1")
        .env("CDC_SPOOL_CHILD", "1")
        .env("CDC_SPOOL_CHECKPOINT", checkpoint)
        .env("CDC_SPOOL_MARK", &mark)
        .env("CDC_SPOOL_DIR", &dir)
        .env("CDC_SPOOL_FEED", &feed_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn");
    Child { root, dir, child }
}

fn wait_mark(root: &Path) {
    let mark = root.join("mark");
    for _ in 0..600 {
        if mark.is_file() {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let err = fs::read_to_string(root.join("child.err")).unwrap_or_default();
    panic!("mark missing: {err}");
}

fn assert_not_closed(dir: &Path) {
    assert!(!dir.join("SEAL").exists());
    assert!(!dir.join("ROLLED_BACK").exists());
    match recover(dir).expect("recover") {
        Recovery::Incomplete(pending) => {
            assert!(!pending.ack_allowed);
            assert!(!pending.target_applied);
            assert!(!pending.cross_table_consistent);
        }
        other => panic!("closed or unknown: {other:?}"),
    }
}

#[test]
fn sigkill_before_chunk_sync_is_incomplete() {
    let mut held = spawn("before_chunk_sync");
    wait_mark(&held.root);
    held.child.kill().expect("kill");
    let status = held.child.wait().expect("wait");
    assert_eq!(status.signal(), Some(9), "{status:?}");
    assert!(held.dir.join("chunk-000001.partial").is_file());
    assert!(!held.dir.join("chunk-000001").exists());
    assert!(!held.dir.join("manifest.json").exists());
    let partial = fs::read(held.dir.join("chunk-000001.partial")).unwrap();
    assert!(!partial.is_empty());
    assert_not_closed(&held.dir);
}

#[test]
fn sigkill_before_end_keeps_synced_chunks() {
    let mut held = spawn("before_end");
    wait_mark(&held.root);
    held.child.kill().expect("kill");
    let status = held.child.wait().expect("wait");
    assert_eq!(status.signal(), Some(9), "{status:?}");
    assert!(held.dir.join("chunk-000001").is_file());
    assert!(held.dir.join("chunk-000002").is_file());
    assert!(held.dir.join("chunks.idx").is_file());
    assert!(!held.dir.join("manifest.json").exists());
    assert_not_closed(&held.dir);
    let Recovery::Incomplete(pending) = recover(&held.dir).unwrap() else {
        panic!("incomplete");
    };
    assert_eq!(pending.durable_events, 2);
    assert!(!pending.ack_allowed);
}

#[test]
fn sigkill_before_final_seal_keeps_manifest_without_seal() {
    let mut held = spawn("before_final_seal");
    wait_mark(&held.root);
    held.child.kill().expect("kill");
    let status = held.child.wait().expect("wait");
    assert_eq!(status.signal(), Some(9), "{status:?}");
    assert!(held.dir.join("manifest.json").is_file());
    assert!(!held.dir.join("SEAL").exists());
    let text = fs::read_to_string(held.dir.join("manifest.json")).unwrap();
    assert!(text.contains("\"ack_allowed\":false"));
    assert_not_closed(&held.dir);
}

#[test]
fn checkpoint_continue_seals() {
    let mut held = spawn("before_final_seal");
    wait_mark(&held.root);
    fs::write(held.root.join("mark.continue"), b"go\n").unwrap();
    let status = held.child.wait().expect("wait");
    assert_eq!(status.code(), Some(0), "{status:?}");
    assert!(held.dir.join("SEAL").is_file());
    assert!(!held.dir.join("ROLLED_BACK").exists());
    let Recovery::Committed(committed) = recover(&held.dir).unwrap() else {
        panic!("committed");
    };
    assert_eq!(committed.manifest.count, 2);
    assert!(!committed.ack_allowed());
    assert_eq!(committed.manifest.chunks.len(), 2);
}
