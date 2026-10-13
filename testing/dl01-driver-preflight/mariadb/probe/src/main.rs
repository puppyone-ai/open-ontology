#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use mariadb_probe::audit;
use mariadb_probe::fail::Fail;
use mariadb_probe::session::{self, Command, Request};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mariadb-probe: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), Fail> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("audit") {
        return audit_file(&args);
    }
    let request = parse(&args)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|err| Fail::new("runtime", err.to_string()))?;
    runtime.block_on(session::execute(request))
}

fn audit_file(args: &[String]) -> Result<(), Fail> {
    let mut log = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--log" => {
                log = Some(need(args, &mut index)?);
            }
            other => return Err(Fail::new("argv", other.to_string())),
        }
        index += 1;
    }
    let path = log.ok_or_else(|| Fail::new("argv", "--log"))?;
    let text = fs::read_to_string(&path).map_err(|err| Fail::new("audit", err.to_string()))?;
    match audit::audit_log(&text) {
        Ok(counts) => {
            print_counts(&counts);
            Ok(())
        }
        Err(err) => {
            eprintln!("{err}");
            Err(err)
        }
    }
}

fn print_counts(counts: &audit::AuditCounts) {
    println!(
        "rows={} connect={} read={} session={} cancel={} execute={} write={} empty={} unexpected={}",
        counts.rows,
        counts.connect,
        counts.read,
        counts.session,
        counts.cancel,
        counts.execute,
        counts.write,
        counts.empty,
        counts.unexpected
    );
}

fn parse(args: &[String]) -> Result<Request, Fail> {
    if args.is_empty() {
        return Err(Fail::new("argv", "missing command"));
    }
    let command = match args[0].as_str() {
        "read" => Command::Read,
        "reject-table" => Command::RejectTable,
        "zero-date" => Command::ZeroDate,
        "deny-select" => Command::DenySelect,
        "cancel" => Command::Cancel,
        "broken" => Command::Broken,
        "caps" => Command::Read,
        other => return Err(Fail::new("argv", other.to_string())),
    };
    let mut command = command;
    let mut host = None;
    let mut port = 3306u16;
    let mut user = None;
    let mut password_file = None;
    let mut out = None;
    let mut table = None;
    let mut ready_file = None;
    let mut go_file = None;
    let mut max_rows = 1000u64;
    let mut max_bytes = 8 * 1024 * 1024u64;
    let mut deadline_ms = 60_000u64;
    let mut batch_rows = 4usize;
    let mut image_digest = session::IMAGE_DIGEST.to_string();
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        match flag {
            "--host" => host = Some(need(args, &mut index)?),
            "--port" => port = parse_num(&need(args, &mut index)?, "port")?,
            "--user" => user = Some(need(args, &mut index)?),
            "--password-file" => password_file = Some(PathBuf::from(need(args, &mut index)?)),
            "--out" => out = Some(PathBuf::from(need(args, &mut index)?)),
            "--table" => table = Some(need(args, &mut index)?),
            "--ready-file" => ready_file = Some(PathBuf::from(need(args, &mut index)?)),
            "--go-file" => go_file = Some(PathBuf::from(need(args, &mut index)?)),
            "--max-rows" => max_rows = parse_num(&need(args, &mut index)?, "max-rows")?,
            "--max-bytes" => max_bytes = parse_num(&need(args, &mut index)?, "max-bytes")?,
            "--deadline-ms" => deadline_ms = parse_num(&need(args, &mut index)?, "deadline-ms")?,
            "--batch-rows" => batch_rows = parse_num(&need(args, &mut index)?, "batch-rows")?,
            "--image-digest" => image_digest = need(args, &mut index)?,
            "row" if args[0] == "caps" => command = Command::CapsRow,
            "byte" if args[0] == "caps" => command = Command::CapsByte,
            "deadline" if args[0] == "caps" => command = Command::CapsDeadline,
            other => return Err(Fail::new("argv", other.to_string())),
        }
        index += 1;
    }
    if args[0] == "caps" && matches!(command, Command::Read) {
        return Err(Fail::new("argv", "caps mode"));
    }
    Ok(Request {
        host: host.ok_or_else(|| Fail::new("argv", "--host"))?,
        port,
        user: user.ok_or_else(|| Fail::new("argv", "--user"))?,
        password_file: password_file.ok_or_else(|| Fail::new("argv", "--password-file"))?,
        out: out.ok_or_else(|| Fail::new("argv", "--out"))?,
        command,
        table,
        ready_file,
        go_file,
        max_rows,
        max_bytes,
        deadline: Duration::from_millis(deadline_ms),
        batch_rows,
        image_digest,
    })
}

fn need(args: &[String], index: &mut usize) -> Result<String, Fail> {
    *index += 1;
    args.get(*index).cloned().ok_or_else(|| Fail::new("argv", "missing value"))
}

fn parse_num<T: std::str::FromStr>(text: &str, name: &str) -> Result<T, Fail> {
    text.parse().map_err(|_| Fail::new("argv", name.to_string()))
}
