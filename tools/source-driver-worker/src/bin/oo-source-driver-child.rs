//! Fixture child for `oo.source.driver-worker/v1`.
//!
//! This process is not an Oracle, ODBC, or JDBC driver. It hashes the
//! customer artifact and does not load it. A statement other than the fixture
//! names, including `oracle:select-1`, returns `read_unsupported` and does
//! not exec another binary.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use source_driver_worker::{
    completion_text, encode_ipc, file_digest, fingerprint_hex, lineage_text, parse_child,
    rows_batch, write_frame, ChildBody, ARTIFACT_NAME, DRIVER_MAX_BYTES, PROTOCOL,
    REQUEST_MAX_BYTES, TAG_COMPLETE, TAG_IPC, TAG_LINEAGE,
};

const WIDE_BYTES: usize = 2 * 1024 * 1024;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(stop) => {
            let token = stop.token;
            let _ = writeln!(io::stderr(), "oo-source-driver-child: {token}");
            let _ = io::stderr().flush();
            ExitCode::from(stop.code)
        }
    }
}

struct Stop {
    token: &'static str,
    code: u8,
}

impl Stop {
    fn new(token: &'static str) -> Self {
        Self::code(token, if token == "crash" { 2 } else { 1 })
    }

    fn code(token: &'static str, code: u8) -> Self {
        Self { token, code }
    }
}

fn run() -> Result<(), Stop> {
    let mut args = env::args().skip(1);
    match (args.next(), args.next()) {
        (None, _) => drive(),
        (Some(flag), None) if flag == "--protocol" => {
            println!("{PROTOCOL}");
            let _ = io::stdout().flush();
            Ok(())
        }
        _ => Err(Stop::code("protocol", 2)),
    }
}

fn drive() -> Result<(), Stop> {
    let bytes = read_stdin()?;
    let body = parse_child(&bytes).map_err(config_stop)?;
    let exe = env::current_exe().map_err(|_| Stop::new("image_digest"))?;
    let image = file_digest(&exe, DRIVER_MAX_BYTES).map_err(|_| Stop::new("image_digest"))?;
    if image != body.image_digest {
        return Err(Stop::new("image_digest"));
    }
    check_artifact(&body)?;
    dispatch(&body)
}

fn read_stdin() -> Result<Vec<u8>, Stop> {
    let mut stdin = io::stdin();
    let mut bytes = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = stdin.read(&mut buf).map_err(|_| Stop::new("io"))?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) > REQUEST_MAX_BYTES {
            return Err(Stop::new("request_too_large"));
        }
        bytes.extend_from_slice(&buf[..read]);
    }
    Ok(bytes)
}

fn check_artifact(body: &ChildBody) -> Result<(), Stop> {
    let path = Path::new(&body.volume).join(ARTIFACT_NAME);
    let meta = fs::symlink_metadata(&path).map_err(|_| Stop::new("DRIVER_MISSING"))?;
    if !meta.file_type().is_file() {
        return Err(Stop::new("DRIVER_MISSING"));
    }
    let digest = file_digest(&path, DRIVER_MAX_BYTES).map_err(|_| Stop::new("driver_digest"))?;
    if digest != body.driver_digest {
        return Err(Stop::new("driver_digest"));
    }
    Ok(())
}

fn dispatch(body: &ChildBody) -> Result<(), Stop> {
    match body.statement_ref.as_str() {
        "fixture:two-rows" => {
            let rows = [(1, Some("alpha")), (2, None)];
            emit(body, &rows, false)
        }
        "fixture:wide" => {
            let blob = "W".repeat(WIDE_BYTES);
            let rows: Vec<(i64, Option<&str>)> =
                (0..4).map(|index| (index, Some(blob.as_str()))).collect();
            emit(body, &rows, true)
        }
        "fixture:slow" => {
            let start = Instant::now();
            let limit = Duration::from_millis(body.deadline_ms);
            while start.elapsed() < limit {
                thread::sleep(Duration::from_millis(20));
            }
            Err(Stop::new("deadline"))
        }
        "fixture:crash" => Err(Stop::new("crash")),
        "fixture:malformed" => {
            let mut out = io::stdout().lock();
            out.write_all(&[0xff; 9]).map_err(|_| Stop::new("io"))?;
            out.flush().map_err(|_| Stop::new("io"))?;
            Ok(())
        }
        _ => Err(Stop::new("read_unsupported")),
    }
}

fn emit(body: &ChildBody, rows: &[(i64, Option<&str>)], progress: bool) -> Result<(), Stop> {
    if rows.is_empty() || rows.len() as u64 > body.max_rows {
        return Err(Stop::new("budget"));
    }
    let fingerprint = fingerprint_hex().map_err(config_stop)?;
    let cap = usize::try_from(body.batch_bytes).map_err(|_| Stop::new("budget"))?;
    let chunk = usize::try_from(body.batch_rows).map_err(|_| Stop::new("budget"))?;
    if chunk == 0 {
        return Err(Stop::new("budget"));
    }
    let mut index = 0u64;
    let mut total = 0u64;
    for group in rows.chunks(chunk) {
        let batch = rows_batch(group).map_err(config_stop)?;
        let (ipc, _) = encode_ipc(&batch, cap).map_err(config_stop)?;
        let lineage = lineage_text(index, group.len() as u64, ipc.len(), &fingerprint);
        let mut out = io::stdout().lock();
        write_frame(&mut out, TAG_IPC, &ipc).map_err(|_| Stop::new("io"))?;
        write_frame(&mut out, TAG_LINEAGE, lineage.as_bytes()).map_err(|_| Stop::new("io"))?;
        out.flush().map_err(|_| Stop::new("io"))?;
        drop(out);
        total = total.saturating_add(group.len() as u64);
        index = index.saturating_add(1);
        if progress {
            fs::write("progress", total.to_string()).map_err(|_| Stop::new("io"))?;
        }
    }
    let done = completion_text(
        &body.statement_ref,
        total,
        index,
        &fingerprint,
        &body.driver_digest,
        &body.image_digest,
    );
    let mut out = io::stdout().lock();
    write_frame(&mut out, TAG_COMPLETE, done.as_bytes()).map_err(|_| Stop::new("io"))?;
    out.flush().map_err(|_| Stop::new("io"))?;
    Ok(())
}

fn config_stop(err: source_driver_worker::ParentError) -> Stop {
    match err {
        source_driver_worker::ParentError::Config(token) => Stop::new(token),
        source_driver_worker::ParentError::Failed(_) => Stop::new("protocol"),
    }
}
