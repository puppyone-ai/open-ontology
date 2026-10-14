use std::env;
use std::path::Path;
use std::process::ExitCode;

use doris_reader::fail::Fail;
use doris_reader::session::{evidence_dir, ident, ipv4, Endpoint};
use doris_reader::{capture, dba, gate};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("dba-setup") if args.len() == 9 => run_dba(&args),
        Some("reader-gate") if args.len() == 6 => run_gate(&args),
        Some("capture-tick") if args.len() == 7 => run_capture_tick(&args),
        Some("capture-connection") if args.len() == 10 => run_connection(&args, false),
        Some("capture-connection-partition") if args.len() == 11 => run_connection(&args, true),
        Some("capture-tick-budget") if args.len() == 8 => run_capture_tick_budget(&args),
        Some("capture-full-baseline-budget") if args.len() == 8 => run_capture_full_baseline_budget(&args),
        Some("capture-partition-budget") if args.len() == 9 => run_capture_partition_budget(&args),
        Some("capture-dba") if args.len() == 6 => run_capture_dba(&args),
        _ => {
            eprintln!(
                "usage: doris-reader dba-setup <evidence> <sql> <secrets> <host> <port> <db> <be-host> <be-port>"
            );
            eprintln!("       doris-reader reader-gate <evidence> <secrets> <host> <port> <db>");
            eprintln!("       doris-reader capture-tick <evidence> <secrets> <host> <port> <every_ms> <label>");
            eprintln!("       doris-reader capture-connection <evidence> <secrets> <host> <port> <database> <user> <every_ms> <label> <max_rows_per_table>");
            eprintln!("       doris-reader capture-connection-partition <evidence> <secrets> <host> <port> <database> <user> <every_ms> <label> <max_rows> <partition>");
            eprintln!("       doris-reader capture-tick-budget <evidence> <secrets> <host> <port> <every_ms> <label> <max_rows_per_table>");
            eprintln!("       doris-reader capture-full-baseline-budget <evidence> <secrets> <host> <port> <every_ms> <label> <max_rows_per_table>");
            eprintln!("       doris-reader capture-partition-budget <evidence> <secrets> <host> <port> <every_ms> <label> <partition> <max_rows>");
            eprintln!("       doris-reader capture-dba <evidence> <secrets> <host> <port> <sql-file>");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) if err.code == "schedule" => {
            eprintln!("{err}");
            ExitCode::from(3)
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn run_capture_tick_budget(args: &[String]) -> Result<(), Fail> {
    let evidence = evidence_dir(&args[1])?;
    let endpoint = capture_endpoint(&args[3], &args[4])?;
    let every_ms = args[5].parse().map_err(|_| Fail::new("schedule", "every_ms"))?;
    let max_rows = args[7].parse().map_err(|_| Fail::new("budget", "max_rows"))?;
    let runtime = runtime()?;
    runtime.block_on(capture::capture_tick_budget(evidence, &args[2], endpoint, every_ms, &args[6], max_rows))
}

fn run_capture_full_baseline_budget(args: &[String]) -> Result<(), Fail> {
    let evidence = evidence_dir(&args[1])?;
    let endpoint = capture_endpoint(&args[3], &args[4])?;
    let every_ms = args[5].parse().map_err(|_| Fail::new("schedule", "every_ms"))?;
    let max_rows = args[7].parse().map_err(|_| Fail::new("budget", "max_rows"))?;
    let runtime = runtime()?;
    runtime.block_on(capture::capture_full_baseline_budget(evidence, &args[2], endpoint, every_ms, &args[6], max_rows))
}

fn run_capture_partition_budget(args: &[String]) -> Result<(), Fail> {
    let evidence = evidence_dir(&args[1])?;
    let endpoint = capture_endpoint(&args[3], &args[4])?;
    let every_ms = args[5].parse().map_err(|_| Fail::new("schedule", "every_ms"))?;
    let max_rows = args[8].parse().map_err(|_| Fail::new("budget", "max_rows"))?;
    let runtime = runtime()?;
    runtime.block_on(capture::capture_partition_budget(
        evidence, &args[2], endpoint, every_ms, &args[6], &args[7], max_rows,
    ))
}

fn run_capture_tick(args: &[String]) -> Result<(), Fail> {
    let evidence = evidence_dir(&args[1])?;
    let endpoint = capture_endpoint(&args[3], &args[4])?;
    let every_ms = args[5].parse().map_err(|_| Fail::new("schedule", "every_ms"))?;
    let runtime = runtime()?;
    runtime.block_on(capture::capture_tick(evidence, &args[2], endpoint, every_ms, &args[6]))
}

fn run_capture_dba(args: &[String]) -> Result<(), Fail> {
    let evidence = evidence_dir(&args[1])?;
    let endpoint = capture_endpoint(&args[3], &args[4])?;
    let runtime = runtime()?;
    runtime.block_on(capture::capture_dba(evidence, &args[2], endpoint, Path::new(&args[5])))
}

fn capture_endpoint(host: &str, port: &str) -> Result<Endpoint, Fail> {
    ipv4(host)?;
    let port = port.parse().map_err(|_| Fail::new("port", "query"))?;
    ident("oo_dl01w1_43")?;
    Ok(Endpoint {
        host: host.to_string(),
        port,
        db: "oo_dl01w1_43".to_string(),
        reader_user: "oo_reader".to_string(),
    })
}

fn run_dba(args: &[String]) -> Result<(), Fail> {
    let evidence = evidence_dir(&args[1])?;
    let endpoint = endpoint(&args[4], &args[5], &args[6])?;
    let be_port: u16 = args[8].parse().map_err(|_| Fail::new("port", "be"))?;
    ipv4(&args[7])?;
    let runtime = runtime()?;
    runtime.block_on(dba::setup(
        evidence,
        args[2].as_str(),
        args[3].as_str(),
        endpoint,
        args[7].as_str(),
        be_port,
    ))
}

fn run_gate(args: &[String]) -> Result<(), Fail> {
    let evidence = evidence_dir(&args[1])?;
    let endpoint = endpoint(&args[3], &args[4], &args[5])?;
    let runtime = runtime()?;
    runtime.block_on(gate::run(evidence, args[2].as_str(), endpoint))
}

fn endpoint(host: &str, port: &str, db: &str) -> Result<Endpoint, Fail> {
    ipv4(host)?;
    ident(db)?;
    if db != "oo_dl01w1_43" {
        return Err(Fail::new("db", "owned database is oo_dl01w1_43"));
    }
    let port = port.parse().map_err(|_| Fail::new("port", "query"))?;
    Ok(Endpoint {
        host: host.to_string(),
        port,
        db: db.to_string(),
        reader_user: "oo_reader".to_string(),
    })
}

fn runtime() -> Result<tokio::runtime::Runtime, Fail> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|err| Fail::new("runtime", err.to_string()))
}


fn connection_endpoint(host: &str, port: &str, db: &str, user: &str) -> Result<Endpoint, Fail> {
    ipv4(host)?;
    ident(db)?;
    ident(user)?;
    let port: u16 = port.parse().map_err(|_| Fail::new("port", "query"))?;
    if port == 0 { return Err(Fail::new("port", "zero")); }
    Ok(Endpoint { host: host.to_string(), port, db: db.to_string(), reader_user: user.to_string() })
}

fn run_connection(args: &[String], partition: bool) -> Result<(), Fail> {
    let evidence = evidence_dir(&args[1])?;
    let endpoint = connection_endpoint(&args[3], &args[4], &args[5], &args[6])?;
    let period = args[7].parse().map_err(|_| Fail::new("schedule", "every_ms"))?;
    let max_rows = args[9].parse().map_err(|_| Fail::new("budget", "max_rows"))?;
    let runtime = runtime()?;
    if partition {
        runtime.block_on(capture::capture_partition_budget(evidence, &args[2], endpoint, period, &args[8], &args[10], max_rows))
    } else {
        runtime.block_on(capture::capture_full_baseline_budget(evidence, &args[2], endpoint, period, &args[8], max_rows))
    }
}

#[cfg(test)]
mod connection_tests {
    use super::connection_endpoint;
    #[test]
    fn configured_identity_and_unsafe_names() {
        let e = connection_endpoint("172.16.143.10", "9030", "customer_db", "customer_reader").unwrap();
        assert_eq!(e.db, "customer_db");
        assert_eq!(e.reader_user, "customer_reader");
        assert!(connection_endpoint("127.0.0.1", "0", "db", "reader").is_err());
        assert!(connection_endpoint("127.0.0.1", "9030", "db';DROP", "reader").is_err());
    }
}
