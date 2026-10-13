#![forbid(unsafe_code)]

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use starrocks_probe::fail::Fail;
use starrocks_probe::session::{self, Command, Request};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) if err.code == "gate_pending" => {
            eprintln!("starrocks-probe: {err}");
            ExitCode::from(3)
        }
        Err(err) => {
            eprintln!("starrocks-probe: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), Fail> {
    let request = parse(&env::args().skip(1).collect::<Vec<_>>())?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|err| Fail::new("runtime", err.to_string()))?;
    runtime.block_on(session::execute(request))
}

fn parse(args: &[String]) -> Result<Request, Fail> {
    if args.is_empty() {
        return Err(Fail::new("argv", "missing command"));
    }
    let mut command = match args[0].as_str() {
        "read" => Command::Read,
        "partition-read" => Command::PartitionRead,
        "block" => Command::Block,
        "caps" => Command::CapsRow,
        "deadline" => Command::Deadline,
        "cancel" => Command::Cancel,
        other => return Err(Fail::new("argv", other.to_string())),
    };
    let mut host = None;
    let mut port = 9030u16;
    let mut password_file = None;
    let mut out = None;
    let mut image_digest = None;
    let mut max_rows = 1000u64;
    let mut max_bytes = 8 * 1024 * 1024u64;
    let mut deadline_ms = 60_000u64;
    let mut partition = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--host" => host = Some(need(args, &mut index)?),
            "--port" => port = parse_num(&need(args, &mut index)?, "port")?,
            "--password-file" => password_file = Some(PathBuf::from(need(args, &mut index)?)),
            "--out" => out = Some(PathBuf::from(need(args, &mut index)?)),
            "--image-digest" => image_digest = Some(need(args, &mut index)?),
            "--max-rows" => max_rows = parse_num(&need(args, &mut index)?, "max-rows")?,
            "--max-bytes" => max_bytes = parse_num(&need(args, &mut index)?, "max-bytes")?,
            "--deadline-ms" => deadline_ms = parse_num(&need(args, &mut index)?, "deadline-ms")?,
            "--partition" => partition = Some(need(args, &mut index)?),
            "row" if args[0] == "caps" => command = Command::CapsRow,
            "byte" if args[0] == "caps" => command = Command::CapsByte,
            other => return Err(Fail::new("argv", other.to_string())),
        }
        index += 1;
    }
    if args[0] == "caps" && !args.iter().any(|item| item == "row" || item == "byte") {
        return Err(Fail::new("argv", "caps requires row or byte"));
    }
    Ok(Request {
        command,
        partition,
        host: host.ok_or_else(|| Fail::new("argv", "--host"))?,
        port,
        password_file: password_file.ok_or_else(|| Fail::new("argv", "--password-file"))?,
        out: out.ok_or_else(|| Fail::new("argv", "--out"))?,
        image_digest: image_digest.ok_or_else(|| Fail::new("argv", "--image-digest"))?,
        max_rows,
        max_bytes,
        deadline: Duration::from_millis(deadline_ms),
    })
}

fn need(args: &[String], index: &mut usize) -> Result<String, Fail> {
    *index += 1;
    args.get(*index).cloned().ok_or_else(|| Fail::new("argv", "missing value"))
}

fn parse_num<T: std::str::FromStr>(text: &str, name: &str) -> Result<T, Fail> {
    text.parse::<T>().map_err(|_| Fail::new("argv", name))
}
