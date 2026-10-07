#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use local_pg_mirror::{
    copy, discover, read_local, CopyRequest, CopyStop, DiscoverRequest, MirrorError, ReadRequest,
};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("{err}");
            exit_for(&err)
        }
    }
}

fn run() -> Result<ExitCode, MirrorError> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("discover") => {
            let config_path = path(&mut args)?;
            let plan_path = path(&mut args)?;
            let source_password = path(&mut args)?;
            let source_ca = path(&mut args)?;
            if args.next().is_some() {
                return Err(MirrorError::new("config", "usage"));
            }
            let plan = tokio_run(discover(DiscoverRequest {
                config_path,
                plan_path,
                source_password,
                source_ca,
            }))?;
            println!("planned 0 {} none false", plan.plan_sha256);
            Ok(ExitCode::SUCCESS)
        }
        Some("copy") => {
            let plan_path = path(&mut args)?;
            let source_password = path(&mut args)?;
            let source_ca = path(&mut args)?;
            let admin_password = path(&mut args)?;
            let runtime_password = path(&mut args)?;
            let manifest_path = path(&mut args)?;
            let target_config_path = path(&mut args)?;
            let target_ca = path(&mut args)?;
            let stop = match args.next().as_deref() {
                Some("finish") => CopyStop::Finish,
                Some("cancel") => CopyStop::CancelBeforePull,
                Some("stop-after") => {
                    let raw = args.next().ok_or_else(|| MirrorError::new("config", "usage"))?;
                    let count: u64 = raw.parse().map_err(|_| MirrorError::new("config", "usage"))?;
                    CopyStop::StopAfterBatches(count)
                }
                _ => return Err(MirrorError::new("config", "usage")),
            };
            if args.next().is_some() {
                return Err(MirrorError::new("config", "usage"));
            }
            let outcome = tokio_run(copy(CopyRequest {
                plan_path,
                source_password,
                source_ca,
                admin_password,
                runtime_password,
                manifest_path,
                target_config_path,
                target_ca,
                stop,
            }))?;
            let failure = if outcome.disposition == "replay_complete" {
                "replay_complete"
            } else {
                outcome.manifest.failure.as_str()
            };
            println!(
                "{} {} {} {} false",
                outcome.manifest.mirror_state, outcome.manifest.rows, outcome.manifest.plan_sha256, failure
            );
            Ok(exit_state(&outcome.manifest.mirror_state))
        }
        Some("read-local") => {
            let target_config = path(&mut args)?;
            let manifest = path(&mut args)?;
            let reader_password = path(&mut args)?;
            let ca = path(&mut args)?;
            if args.next().is_some() {
                return Err(MirrorError::new("config", "usage"));
            }
            let read = tokio_run(read_local(ReadRequest {
                target_config,
                manifest,
                reader_password,
                ca,
            }))?;
            let rows: usize = read.tables.iter().map(|table| table.rows.len()).sum();
            println!("complete {rows} local none false");
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(MirrorError::new("config", "usage")),
    }
}

fn path(args: &mut impl Iterator<Item = String>) -> Result<PathBuf, MirrorError> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| MirrorError::new("config", "usage"))
}

fn tokio_run<T>(future: impl std::future::Future<Output = Result<T, MirrorError>>) -> Result<T, MirrorError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(1)
        .build()
        .map_err(|_| MirrorError::new("config", "runtime"))?;
    runtime.block_on(future)
}

fn exit_state(state: &str) -> ExitCode {
    match state {
        "complete" | "planned" => ExitCode::SUCCESS,
        "incomplete" => ExitCode::from(2),
        "refused" => ExitCode::from(3),
        _ => ExitCode::from(1),
    }
}

fn exit_for(err: &MirrorError) -> ExitCode {
    match err.code {
        "resume_refused" => ExitCode::from(4),
        "incomplete" => ExitCode::from(2),
        "refused" | "unsupported_engine" | "unsupported_column" | "catalog_drift" | "identity_drift"
        | "plan_tamper" | "ca_drift" | "manifest_conflict" | "plan_exists" => ExitCode::from(3),
        _ => ExitCode::from(1),
    }
}
