//! `mysql-preview run --config PATH --out DIR`
//! `mysql-preview manifest --access PATH --seal PATH --out PATH`

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use mysql_preview::{
    baseline_stream, capture_native, audit_source, audit_target, guard_source, install_column_acl, load_fixture, load_logistics, prepare_random_gold, run, run_customer, write_manifest,
};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    match command.as_str() {
        "capture-native" => capture_cmd(&mut args),
        "manifest" => manifest(&mut args),
        "run" => run_cmd(&mut args),
        "customer" => customer_cmd(&mut args),
        "dba-load" => dba_load(&mut args),
        "dba-fixture" => fixture_cmd(&mut args),
        "install-column-acl" => column_acl_cmd(&mut args),
        "scale-audit" => scale_audit(&mut args),
        "scale-target" => scale_target(&mut args),
        "scale-guard" => scale_guard(&mut args),
        "c1-dba" => c1_dba(&mut args),
        "c1-prove" => c1_prove(&mut args),
        "c1-expire" => c1_expire(&mut args),
        "c1-native" => c1_native(&mut args),
        "c1-onboarding-prepare" => c1_onboarding_prepare(&mut args),
        "c1-baseline-stream" => c1_baseline_stream(&mut args),
        "c1-random-gold" => c1_random_gold(&mut args),
        _ => {
            eprintln!("mysql-preview: usage");
            ExitCode::from(2)
        }
    }
}

fn c1_onboarding_prepare(args: &mut impl Iterator<Item=String>)->ExitCode {
    let (Some(config),Some(out))=pair(args) else {return ExitCode::from(2);};
    block_on(mysql_preview::prepare_onboarding(&config,&out))
}

fn c1_baseline_stream(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(baseline_stream(&config, &out))
}

fn run_cmd(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let mut config = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => {
                eprintln!("mysql-preview: usage");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(config), Some(out)) = (config, out) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(run(&config, &out))
}

fn customer_cmd(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(run_customer(&config, &out))
}

fn fixture_cmd(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(load_fixture(&config, &out))
}

fn column_acl_cmd(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(install_column_acl(&config, &out))
}

fn pair(args: &mut impl Iterator<Item = String>) -> (Option<PathBuf>, Option<PathBuf>) {
    let mut config = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => return (None, None),
        }
    }
    (config, out)
}

fn dba_load(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let mut config = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => {
                eprintln!("mysql-preview: usage");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(config), Some(out)) = (config, out) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(load_logistics(&config, &out))
}

fn scale_audit(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let mut config = None;
    let mut out = None;
    let mut previous = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            ("--previous", Some(path)) => previous = Some(PathBuf::from(path)),
            _ => {
                eprintln!("mysql-preview: usage");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(config), Some(out)) = (config, out) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(audit_source(&config, &out, previous.as_deref()))
}

fn scale_target(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let mut config = None;
    let mut outcome = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--outcome", Some(path)) => outcome = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => {
                eprintln!("mysql-preview: usage");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(config), Some(outcome), Some(out)) = (config, outcome, out) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(audit_target(&config, &outcome, &out))
}

fn scale_guard(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let mut config = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => {
                eprintln!("mysql-preview: usage");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(config), Some(out)) = (config, out) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(guard_source(&config, &out))
}

fn block_on(work: impl std::future::Future<Output = Result<(), mysql_preview::PreviewError>>) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("mysql-preview: io");
            return ExitCode::from(1);
        }
    };
    match runtime.block_on(work) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mysql-preview: {} {}", err.code, err.fact);
            ExitCode::from(exit_code(err.code))
        }
    }
}

fn manifest(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let mut access = None;
    let mut seal = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--access", Some(path)) => access = Some(PathBuf::from(path)),
            ("--seal", Some(path)) => seal = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => {
                eprintln!("mysql-preview: usage");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(access), Some(seal), Some(out)) = (access, seal, out) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    match write_manifest(&access, &seal, &out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mysql-preview: {} {}", err.code, err.fact);
            ExitCode::from(exit_code(err.code))
        }
    }
}

fn exit_code(code: &str) -> u8 {
    match code {
        "config" | "budget" | "ident" => 3,
        "connect" | "tls" => 4,
        "privilege" | "session" => 5,
        "version" => 6,
        "catalog" => 7,
        "value" | "batch" | "protocol" => 8,
        "target" => 9,
        _ => 1,
    }
}

fn capture_cmd(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else { return ExitCode::from(2); };
    block_on(capture_native(&config, &out))
}

fn c1_dba(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else { return ExitCode::from(2); };
    block_on(mysql_preview::configure_dba(&config, &out))
}

fn c1_prove(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else { return ExitCode::from(2); };
    block_on(mysql_preview::prove_c1(&config, &out))
}

fn c1_native(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else { return ExitCode::from(2); };
    block_on(mysql_preview::capture_business(&config, &out))
}

fn c1_random_gold(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else { return ExitCode::from(2); };
    block_on(prepare_random_gold(&config, &out))
}

fn c1_expire(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(from), Some(out)) = c1_expire_args(args) else { return ExitCode::from(2); };
    block_on(mysql_preview::expire_c1(&from.0, &from.1, &out))
}

fn c1_expire_args(args: &mut impl Iterator<Item = String>) -> (Option<(PathBuf, PathBuf)>, Option<PathBuf>) {
    let mut config = None;
    let mut from = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--from", Some(path)) => from = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => return (None, None),
        }
    }
    match (config, from) {
        (Some(config), Some(from)) => (Some((config, from)), out),
        _ => (None, None),
    }
}
