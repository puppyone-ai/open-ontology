//! Acceptance entry. Exit 0 only when every measured check passed.

#![forbid(unsafe_code)]

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use minimal_loop_acceptance::{measure_kit, run_card, write_fixture, CardOpts};

fn main() -> ExitCode {
    let mut args = env::args();
    let _argv0 = args.next();
    match args.next().as_deref() {
        Some("card") => card(args),
        Some("fixture") => fixture(args),
        Some("kit") => kit(args),
        _ => {
            eprintln!("usage: minimal-loop-acceptance card|fixture|kit ...");
            ExitCode::from(2)
        }
    }
}

fn fixture(mut args: env::Args) -> ExitCode {
    let mut out = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = args.next(),
            _ => {
                eprintln!("unknown argument");
                return ExitCode::from(2);
            }
        }
    }
    let Some(out) = out else {
        eprintln!("usage: minimal-loop-acceptance fixture --out <dir>");
        return ExitCode::from(2);
    };
    match write_fixture(std::path::Path::new(&out)) {
        Ok(()) => ExitCode::from(0),
        Err(error) => {
            eprintln!("fixture error: {error}");
            ExitCode::from(1)
        }
    }
}

fn kit(mut args: env::Args) -> ExitCode {
    let mut root = None;
    let mut out = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = args.next(),
            "--out" => out = args.next(),
            _ => {
                eprintln!("unknown argument");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(root), Some(out)) = (root, out) else {
        eprintln!("usage: minimal-loop-acceptance kit --root <mount> --out <dir>");
        return ExitCode::from(2);
    };
    match measure_kit(std::path::Path::new(&root), std::path::Path::new(&out)) {
        Ok(true) => ExitCode::from(0),
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("kit error: {error}");
            ExitCode::from(1)
        }
    }
}

fn card(mut args: env::Args) -> ExitCode {
    let mut repo = None;
    let mut archive = None;
    let mut out = None;
    let mut pinned = None;
    let mut binary = None;
    let mut manifest = None;
    let mut config = None;
    let mut execute = false;
    let mut pi_commit = None;
    let mut pi_executable = None;
    let mut pi_runtime = None;
    let mut inventory = None;
    let mut kit_root = None;
    let mut sealed_manifest = None;
    let mut import_manifest = None;
    let mut copy_review = None;
    let mut product_note = None;
    let mut fixture_probe = None;
    let mut binding_probe = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repo" => repo = args.next(),
            "--archive" => archive = args.next(),
            "--out" => out = args.next(),
            "--pinned-commit" => pinned = args.next(),
            "--binary" => binary = args.next(),
            "--manifest" => manifest = args.next(),
            "--config" => config = args.next(),
            "--execute" => execute = true,
            "--pi-commit" => pi_commit = args.next(),
            "--pi-executable" => pi_executable = args.next(),
            "--pi-runtime" => pi_runtime = args.next(),
            "--inventory" => inventory = args.next(),
            "--kit-root" => kit_root = args.next(),
            "--sealed-manifest" => sealed_manifest = args.next(),
            "--import-manifest" => import_manifest = args.next(),
            "--copy-review" => copy_review = args.next(),
            "--product-note" => product_note = args.next(),
            "--fixture-probe" => fixture_probe = args.next(),
            "--binding-probe" => binding_probe = args.next(),
            _ => {
                eprintln!("unknown argument");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(repo), Some(archive), Some(out), Some(pinned)) = (repo, archive, out, pinned) else {
        eprintln!("missing required argument");
        return ExitCode::from(2);
    };
    let opts = CardOpts {
        repo: PathBuf::from(repo),
        archive: PathBuf::from(archive),
        out: PathBuf::from(out),
        pinned_commit: pinned,
        binary: binary.map(PathBuf::from),
        manifest: manifest.map(PathBuf::from),
        config: config.map(PathBuf::from),
        execute,
        pi_commit,
        pi_executable: pi_executable.map(PathBuf::from),
        pi_runtime: pi_runtime.map(PathBuf::from),
        inventory: inventory.map(PathBuf::from),
        kit_root: kit_root.map(PathBuf::from),
        sealed_manifest: sealed_manifest.map(PathBuf::from),
        import_manifest: import_manifest.map(PathBuf::from),
        copy_review: copy_review.map(PathBuf::from),
        product_note: product_note.map(PathBuf::from),
        fixture_probe: fixture_probe.map(PathBuf::from),
        binding_probe: binding_probe.map(PathBuf::from),
    };
    match run_card(&opts) {
        Ok(verdict) => {
            println!("{}", verdict.json());
            ExitCode::from(verdict.exit_code)
        }
        Err(error) => {
            eprintln!("card error: {error}");
            ExitCode::from(1)
        }
    }
}
