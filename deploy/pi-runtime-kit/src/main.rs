//! `kit verify`, `kit cmap`, `kit probe`, `kit scan`, and `kit handoff`.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use oo_pi_runtime_kit::cmap;
use oo_pi_runtime_kit::handoff;
use oo_pi_runtime_kit::manifest;
use oo_pi_runtime_kit::probe;
use oo_pi_runtime_kit::scan;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let rest: Vec<String> = args.collect();
    match dispatch(&command, &rest) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("kit: {err}");
            ExitCode::from(1)
        }
    }
}

fn dispatch(command: &str, args: &[String]) -> Result<(), String> {
    match command {
        "verify" => verify(args),
        "cmap" => write_cmap(args),
        "probe" => run_probe(args),
        "scan" => scan_dir(args),
        "handoff" => write_handoff(args),
        _ => Err("usage: kit verify|cmap|probe|scan|handoff".to_owned()),
    }
}

fn verify(args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        return Err("usage: kit verify ROOT MANIFEST".to_owned());
    }
    let text = fs::read_to_string(&args[1]).map_err(|err| format!("manifest: {err}"))?;
    let count = manifest::verify_manifest(Path::new(&args[0]), &text)?;
    println!("verified={count}");
    Ok(())
}

fn write_cmap(args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        return Err("usage: kit cmap FONT OUT".to_owned());
    }
    let font = fs::read(&args[0]).map_err(|err| format!("font: {err}"))?;
    let spans = cmap::cjk_spans(&font)?;
    if spans.is_empty() {
        return Err("font cmap has no CJK span".to_owned());
    }
    let body = format!("{}\n", cmap::spans_json(&spans));
    manifest::write_bytes(Path::new(&args[1]), body.as_bytes())
}

fn run_probe(args: &[String]) -> Result<(), String> {
    let parsed = probe::parse_args(args)?;
    probe::run(&probe::ProbePaths {
        out: &parsed.out,
        pi: &parsed.pi,
        runtime: &parsed.runtime,
        oo: &parsed.oo,
        pack: &parsed.pack,
        facts: &parsed.facts,
        cjk: &parsed.cjk,
        handoff: &parsed.handoff,
        volume: &parsed.volume,
        mountpoint: &parsed.mountpoint,
    })
}

fn scan_dir(args: &[String]) -> Result<(), String> {
    if args.len() != 1 {
        return Err("usage: kit scan DIR".to_owned());
    }
    scan::scan_tree(Path::new(&args[0]))
}

fn write_handoff(args: &[String]) -> Result<(), String> {
    if args.len() != 6 {
        return Err("usage: kit handoff FACTS CJK PROBE OUT VOLUME MOUNTPOINT".to_owned());
    }
    let facts = fs::read_to_string(&args[0]).map_err(|err| format!("facts: {err}"))?;
    let cjk = fs::read_to_string(&args[1]).map_err(|err| format!("cjk: {err}"))?;
    let probe_text = fs::read_to_string(&args[2]).map_err(|err| format!("probe: {err}"))?;
    let document = handoff::build(&facts, &cjk, &probe_text, &args[4], &args[5])?;
    manifest::write_bytes(Path::new(&args[3]), document.as_bytes())
}
