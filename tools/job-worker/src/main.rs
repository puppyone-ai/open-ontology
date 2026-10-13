#![forbid(unsafe_code)]

use std::path::PathBuf;

use job_worker::cmd::{cancel_dedupe, fence, show_lease, wait, WaitSpec};
use job_worker::config::Config;
use job_worker::error::{Error, Result};
use job_worker::serve::ServeOpts;

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("error={err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut config_path = None;
    let mut command = None;
    let mut rest = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--config" {
            config_path = Some(args.next().ok_or_else(|| Error::new("usage"))?);
        } else if command.is_none() {
            command = Some(arg);
        } else {
            rest.push(arg);
        }
    }
    let config = Config::load(&PathBuf::from(config_path.ok_or_else(|| Error::new("usage"))?))?;
    let flags = flags(&rest)?;
    match command.as_deref() {
        Some("native-install") => job_worker::native_schedule::install(&config).await,
        Some("native-put") => job_worker::native_schedule::put(&config, &required(&flags,"manifest")?).await,
        Some("native-enable") => job_worker::native_schedule::enabled(&config, &required(&flags,"binding")?, required(&flags,"enabled")? == "true").await,
        Some("native-loader-put") => job_worker::native_schedule::loader_put(&config, &required(&flags,"manifest")?).await,
        Some("native-state") => job_worker::native_schedule::state(&config, &required(&flags,"binding")?).await,
        Some("native-fire") => job_worker::native_schedule::fire(&config, &required(&flags,"project")?).await,
        Some("native-step") => job_worker::native_schedule::step(&config, &required(&flags,"project")?, &required(&flags,"worker")?).await,
        Some("native-serve") => job_worker::native_schedule::serve(&config, &required(&flags,"project")?, &required(&flags,"worker")?, required(&flags,"seconds")?.parse().map_err(|_|Error::new("usage"))?).await,
        Some("apply") => job_worker::apply::apply(&config).await,
        Some("reset") => job_worker::apply::reset(&config).await,
        Some("prepare") => job_worker::prepare::prepare(&config).await,
        Some("serve") => {
            let workers = required(&flags, "workers")?.parse::<u16>().map_err(|_| Error::new("usage"))?;
            let barrier_target = required(&flags, "barrier-target")?
                .parse::<usize>()
                .map_err(|_| Error::new("usage"))?;
            job_worker::serve::serve(
                &config,
                ServeOpts {
                    workers,
                    barrier_prefix: required(&flags, "barrier-prefix")?,
                    barrier_target,
                },
            )
            .await
        }
        Some("wait") => {
            let seconds = required(&flags, "seconds")?
                .parse::<i64>()
                .map_err(|_| Error::new("usage"))?;
            wait(&config, wait_spec(&flags)?, seconds).await
        }
        Some("show-lease") => show_lease(&config, &required(&flags, "dedupe")?).await,
        Some("cancel") => cancel_dedupe(&config, &required(&flags, "dedupe")?).await,
        Some("fence") => {
            fence(
                &config,
                &required(&flags, "phase")?,
                &required(&flags, "dedupe")?,
            )
            .await
        }
        Some("report") => {
            let text = job_worker::report::report(&config).await?;
            println!("{text}");
            Ok(())
        }
        Some(command) if command.starts_with("capture-") => {
            job_worker::capture::dispatch(&config, command, &flags).await
        }
        _ => Err(Error::new("usage")),
    }
}

fn flags(rest: &[String]) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut iter = rest.iter();
    while let Some(flag) = iter.next() {
        let Some(name) = flag.strip_prefix("--") else {
            return Err(Error::new("usage"));
        };
        let value = iter.next().ok_or_else(|| Error::new("usage"))?;
        out.push((name.to_string(), value.clone()));
    }
    Ok(out)
}

fn required(flags: &[(String, String)], name: &str) -> Result<String> {
    flags
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| Error::new("usage"))
}

fn wait_spec(flags: &[(String, String)]) -> Result<WaitSpec> {
    match required(flags, "kind")?.as_str() {
        "observation" => Ok(WaitSpec::Observation {
            name: required(flags, "name")?,
            value: required(flags, "value")?
                .parse()
                .map_err(|_| Error::new("usage"))?,
        }),
        "state" => Ok(WaitSpec::State {
            dedupe: required(flags, "dedupe")?,
            state: required(flags, "state")?,
        }),
        "applied" => Ok(WaitSpec::Applied {
            dedupe: required(flags, "dedupe")?,
            at_least: required(flags, "applied")?
                .parse()
                .map_err(|_| Error::new("usage"))?,
        }),
        "beat" => Ok(WaitSpec::Beat {
            dedupe: required(flags, "dedupe")?,
        }),
        "expired" => Ok(WaitSpec::Expired {
            dedupe: required(flags, "dedupe")?,
        }),
        "schedule" => Ok(WaitSpec::Schedule {
            state: required(flags, "state")?,
        }),
        _ => Err(Error::new("usage")),
    }
}
