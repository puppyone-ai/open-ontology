//! Reap one owner deployment. The clock is the process clock. There is no bearer URL.

#![forbid(unsafe_code)]

use std::time::Duration;

use artifact_retention::{Deployment, Now, PauseAfter, report_json};
use oo_control::FileHost;

fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("artifact-retention: {error}");
            1
        }
    };
    std::process::exit(code);
}

fn run() -> Result<i32, artifact_retention::Error> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let parsed = parse(&args)?;
    let deploy = Deployment::open(&parsed.root)?;
    let host = FileHost::open_local(&deploy.filehost_dir(), deploy.limits())
        .map_err(|_| artifact_retention::Error::Store)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .map_err(|_| artifact_retention::Error::Store)?;
    let report = runtime.block_on(deploy.reap(&host, Now::Production, parsed.pause))?;
    if report.paused {
        let flag = deploy.control_dir().join("waiting");
        std::fs::write(&flag, b"waiting\n").map_err(|_| artifact_retention::Error::Store)?;
        let release = deploy.control_dir().join("continue");
        while !release.exists() {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    println!("{}", report_json(&report));
    if report.paused {
        return Ok(75);
    }
    if report.idle {
        return Ok(0);
    }
    Ok(2)
}

struct Parsed {
    root: std::path::PathBuf,
    pause: PauseAfter,
}

fn parse(args: &[String]) -> Result<Parsed, artifact_retention::Error> {
    if args.first().map(String::as_str) != Some("reap") {
        return Err(artifact_retention::Error::Admission);
    }
    let mut root = None;
    let mut pause = PauseAfter::None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--root" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Err(artifact_retention::Error::Admission);
                };
                root = Some(std::path::PathBuf::from(value));
            }
            "--pause-after" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Err(artifact_retention::Error::Admission);
                };
                pause = match value.as_str() {
                    "pre_delete" => PauseAfter::PreDelete,
                    "metadata_api" => PauseAfter::MetadataApi,
                    "none" => PauseAfter::None,
                    _ => return Err(artifact_retention::Error::Admission),
                };
            }
            _ => return Err(artifact_retention::Error::Admission),
        }
        index += 1;
    }
    let Some(root) = root else {
        return Err(artifact_retention::Error::Admission);
    };
    Ok(Parsed { root, pause })
}
