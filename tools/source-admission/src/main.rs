//! `oo-source-admission` reads files. Passwords stay in files, not argv.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

use source_admission::{
    AUDIT_PROTOCOL, CapturedInput, OBSERVE_PROTOCOL, ObserveConfig, OutputPaths, SourceConfig,
    TargetReport, adapt, admit, admit_pg_budget, classify_csv, decide_captured, decision_json,
    observe, recipe_json, registration_recipe, seal_captured, witness_paths,
};
use zeroize::Zeroizing;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(cmd) = args.next() else {
        eprintln!(
            "usage: oo-source-admission validate|seal-native|adapt-pg|admit-pg-budget|audit-log|observe|registration-recipe"
        );
        return ExitCode::from(1);
    };
    match cmd.as_str() {
        "validate" => validate(&collect(args)),
        "seal-native" => seal_native(&collect(args)),
        "adapt-pg" => adapt_pg(&collect(args)),
        "admit-pg-budget" => admit_pg_budget_cmd(&collect(args)),
        "audit-log" => audit_log(&collect(args)),
        "observe" => observe_cmd(&collect(args)),
        "registration-recipe" => registration_recipe_cmd(&collect(args)),
        _ => {
            eprintln!("unknown command");
            ExitCode::from(1)
        }
    }
}

fn collect(args: impl Iterator<Item = String>) -> Vec<String> {
    args.collect()
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn read_file(path: &str) -> Result<Vec<u8>, ()> {
    fs::read(path).map_err(|_| ())
}

fn validate(args: &[String]) -> ExitCode {
    let (Some(report_path), Some(config_path)) = (flag(args, "--report"), flag(args, "--config"))
    else {
        eprintln!("validate requires --report and --config");
        return ExitCode::from(1);
    };
    let report_bytes = match read_file(report_path) {
        Ok(bytes) => bytes,
        Err(()) => {
            eprintln!("report unreadable");
            return ExitCode::from(1);
        }
    };
    let config_bytes = match read_file(config_path) {
        Ok(bytes) => bytes,
        Err(()) => {
            eprintln!("config unreadable");
            return ExitCode::from(1);
        }
    };
    let report: TargetReport = match source_admission::parse_strict(&report_bytes) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("report invalid: {}", err.fact);
            return ExitCode::from(1);
        }
    };
    let config: SourceConfig = match source_admission::parse_strict(&config_bytes) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("config invalid: {}", err.fact);
            return ExitCode::from(1);
        }
    };
    if report.capture_witness.is_some() {
        return validate_captured(&report, &config, &report_bytes, &config_bytes);
    }
    finish_decision(&admit(&report, &config))
}

fn validate_captured(
    report: &TargetReport,
    config: &SourceConfig,
    report_bytes: &[u8],
    config_bytes: &[u8],
) -> ExitCode {
    let Some(witness_path) = report.capture_witness.as_deref() else {
        return ExitCode::from(1);
    };
    let loaded = match load_captured(witness_path) {
        Ok(loaded) => loaded,
        Err(fact) => {
            eprintln!("witness invalid: {fact}");
            return ExitCode::from(1);
        }
    };
    match decide_captured(
        report,
        config,
        &CapturedInput {
            witness: &loaded.witness,
            capturer: &loaded.capturer,
            arrow: &loaded.arrow,
            driver: &loaded.driver,
            schema_preimage: &loaded.schema_preimage,
            type_preimage: &loaded.type_preimage,
            report: report_bytes,
            config: config_bytes,
        },
    ) {
        Ok(decision) => finish_decision(&decision),
        Err(err) => {
            eprintln!("witness invalid: {}", err.fact);
            ExitCode::from(1)
        }
    }
}

struct Loaded {
    witness: Vec<u8>,
    capturer: Vec<u8>,
    arrow: Vec<u8>,
    driver: Vec<u8>,
    schema_preimage: Vec<u8>,
    type_preimage: Vec<u8>,
}

fn load_captured(witness_path: &str) -> Result<Loaded, String> {
    if !absolute_cli(witness_path) {
        return Err("path".to_string());
    }
    let witness = fs::read(witness_path).map_err(|_| "witness unreadable".to_string())?;
    let files = witness_paths(&witness).map_err(|err| err.fact)?;
    Ok(Loaded {
        capturer: fs::read(&files.capturer_path).map_err(|_| "capturer unreadable".to_string())?,
        arrow: fs::read(&files.arrow_path).map_err(|_| "arrow unreadable".to_string())?,
        driver: fs::read(&files.driver_path).map_err(|_| "driver unreadable".to_string())?,
        schema_preimage: fs::read(&files.schema_preimage_path)
            .map_err(|_| "schema preimage unreadable".to_string())?,
        type_preimage: fs::read(&files.type_preimage_path)
            .map_err(|_| "type preimage unreadable".to_string())?,
        witness,
    })
}

fn absolute_cli(path: &str) -> bool {
    path.starts_with('/') && !path.contains('\0') && !path.contains('\n')
}

fn registration_recipe_cmd(args: &[String]) -> ExitCode {
    let (Some(validator), Some(driver)) = (flag(args, "--validator"), flag(args, "--driver"))
    else {
        eprintln!("registration-recipe requires --validator and --driver");
        return ExitCode::from(1);
    };
    if !absolute_cli(validator) || !absolute_cli(driver) {
        eprintln!("path");
        return ExitCode::from(1);
    }
    let validator_bytes = match read_file(validator) {
        Ok(bytes) => bytes,
        Err(()) => {
            eprintln!("validator unreadable");
            return ExitCode::from(1);
        }
    };
    let driver_bytes = match read_file(driver) {
        Ok(bytes) => bytes,
        Err(()) => {
            eprintln!("driver unreadable");
            return ExitCode::from(1);
        }
    };
    let bytes = recipe_json(&registration_recipe(&validator_bytes, &driver_bytes));
    let _ = std::io::Write::write_all(&mut std::io::stdout(), &bytes);
    ExitCode::from(0)
}

fn seal_native(args: &[String]) -> ExitCode {
    let (Some(report_path), Some(config_path), Some(out)) = (
        flag(args, "--report"),
        flag(args, "--config"),
        flag(args, "--out"),
    ) else {
        eprintln!("seal-native requires --report --config --out");
        return ExitCode::from(1);
    };
    if !absolute_cli(report_path) || !absolute_cli(config_path) || !absolute_cli(out) {
        eprintln!("path");
        return ExitCode::from(1);
    }
    let out_path = std::path::Path::new(out);
    if out_path.exists() {
        eprintln!("output exists");
        return ExitCode::from(1);
    }
    let Some(parent) = out_path.parent().filter(|path| path.is_dir()) else {
        eprintln!("path");
        return ExitCode::from(1);
    };
    let parent = match fs::canonicalize(parent) {
        Ok(path) => path,
        Err(_) => {
            eprintln!("path");
            return ExitCode::from(1);
        }
    };
    let Some(name) = out_path.file_name().and_then(|value| value.to_str()) else {
        eprintln!("path");
        return ExitCode::from(1);
    };
    let final_dir = parent.join(name);
    if final_dir.exists() {
        eprintln!("output exists");
        return ExitCode::from(1);
    }
    let Some(report_path) = canonicalize_file(report_path) else {
        eprintln!("report unreadable");
        return ExitCode::from(1);
    };
    let Some(config_path) = canonicalize_file(config_path) else {
        eprintln!("config unreadable");
        return ExitCode::from(1);
    };
    let report_bytes = match read_file(&report_path) {
        Ok(bytes) => bytes,
        Err(()) => {
            eprintln!("report unreadable");
            return ExitCode::from(1);
        }
    };
    let config_bytes = match read_file(&config_path) {
        Ok(bytes) => bytes,
        Err(()) => {
            eprintln!("config unreadable");
            return ExitCode::from(1);
        }
    };
    let report: TargetReport = match source_admission::parse_strict(&report_bytes) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("report invalid: {}", safe_cli(&err.fact));
            return ExitCode::from(1);
        }
    };
    let config: SourceConfig = match source_admission::parse_strict(&config_bytes) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("config invalid: {}", safe_cli(&err.fact));
            return ExitCode::from(1);
        }
    };
    let Some(witness_path) = report.capture_witness.as_deref() else {
        eprintln!("witness missing");
        return ExitCode::from(1);
    };
    let loaded = match load_captured(witness_path) {
        Ok(loaded) => loaded,
        Err(fact) => {
            eprintln!("witness invalid: {fact}");
            return ExitCode::from(1);
        }
    };
    let catalog_path = path_string(&final_dir.join("catalog.json"));
    let receipt_path = path_string(&final_dir.join("admission-receipt.json"));
    let (Some(catalog_path), Some(receipt_path)) = (catalog_path, receipt_path) else {
        eprintln!("path");
        return ExitCode::from(1);
    };
    let sealed = match seal_captured(
        &report,
        &config,
        &CapturedInput {
            witness: &loaded.witness,
            capturer: &loaded.capturer,
            arrow: &loaded.arrow,
            driver: &loaded.driver,
            schema_preimage: &loaded.schema_preimage,
            type_preimage: &loaded.type_preimage,
            report: &report_bytes,
            config: &config_bytes,
        },
        &OutputPaths {
            report_path: &report_path,
            config_path: &config_path,
            catalog_path: &catalog_path,
            receipt_path: &receipt_path,
        },
    ) {
        Ok(sealed) => sealed,
        Err(err) => {
            eprintln!("witness invalid: {}", err.fact);
            return ExitCode::from(1);
        }
    };
    if !sealed.decision.admitted {
        let _ = std::io::Write::write_all(&mut std::io::stdout(), &sealed.decision_bytes);
        return ExitCode::from(2);
    }
    if let Some(refusal) = sealed.refusal {
        let _ = std::io::Write::write_all(&mut std::io::stdout(), &sealed.decision_bytes);
        eprintln!("{refusal}");
        return ExitCode::from(2);
    }
    let Some(bundle) = sealed.bundle else {
        eprintln!("seal");
        return ExitCode::from(1);
    };
    if !publish(&final_dir, &bundle) {
        eprintln!("output");
        return ExitCode::from(1);
    }
    let _ = std::io::Write::write_all(&mut std::io::stdout(), &bundle.decision);
    ExitCode::from(0)
}

fn canonicalize_file(path: &str) -> Option<String> {
    fs::canonicalize(path)
        .ok()
        .and_then(|path| path.to_str().map(str::to_string))
}

fn path_string(path: &std::path::Path) -> Option<String> {
    path.to_str().map(str::to_string)
}

fn safe_cli(fact: &str) -> String {
    if fact.contains("secret://") {
        return "secret".to_string();
    }
    fact.lines()
        .next()
        .unwrap_or("json")
        .chars()
        .take(180)
        .collect()
}

fn publish(out: &std::path::Path, bundle: &source_admission::SealBundle) -> bool {
    let Some(parent) = out.parent() else {
        return false;
    };
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        out.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("seal"),
        std::process::id()
    ));
    if tmp.exists() {
        let _ = fs::remove_dir_all(&tmp);
    }
    if fs::create_dir(&tmp).is_err() {
        return false;
    }
    let files = [
        ("decision.json", bundle.decision.as_slice()),
        ("admission-receipt.json", bundle.receipt.as_slice()),
        ("registry.json", bundle.registry.as_slice()),
        ("catalog.json", bundle.catalog.as_slice()),
    ];
    for (name, bytes) in files {
        if fs::write(tmp.join(name), bytes).is_err() {
            let _ = fs::remove_dir_all(&tmp);
            return false;
        }
    }
    if fs::rename(&tmp, out).is_err() {
        let _ = fs::remove_dir_all(&tmp);
        return false;
    }
    true
}

fn adapt_pg(args: &[String]) -> ExitCode {
    let Some(report_path) = flag(args, "--report") else {
        eprintln!("adapt-pg requires --report");
        return ExitCode::from(1);
    };
    let bytes = match read_file(report_path) {
        Ok(bytes) => bytes,
        Err(()) => {
            eprintln!("report unreadable");
            return ExitCode::from(1);
        }
    };
    match adapt(&bytes) {
        Ok(decision) => finish_decision(&decision),
        Err(err) => {
            eprintln!("report invalid: {}", err.fact);
            ExitCode::from(1)
        }
    }
}

fn admit_pg_budget_cmd(args: &[String]) -> ExitCode {
    let (Some(evidence_path), Some(access_path), Some(config_path)) = (
        flag(args, "--evidence"),
        flag(args, "--access"),
        flag(args, "--config"),
    ) else {
        eprintln!("admit-pg-budget requires --evidence --access --config");
        return ExitCode::from(1);
    };
    let evidence = match read_file(evidence_path) {
        Ok(bytes) => bytes,
        Err(()) => {
            eprintln!("evidence unreadable");
            return ExitCode::from(1);
        }
    };
    let access = match read_file(access_path) {
        Ok(bytes) => bytes,
        Err(()) => {
            eprintln!("access unreadable");
            return ExitCode::from(1);
        }
    };
    let config = match read_file(config_path) {
        Ok(bytes) => bytes,
        Err(()) => {
            eprintln!("config unreadable");
            return ExitCode::from(1);
        }
    };
    match admit_pg_budget(&evidence, &access, &config) {
        Ok(decision) => finish_decision(&decision),
        Err(err) => {
            eprintln!("report invalid: {}", err.fact);
            ExitCode::from(1)
        }
    }
}

fn audit_log(args: &[String]) -> ExitCode {
    let (Some(reader), Some(path)) = (flag(args, "--reader"), flag(args, "--log")) else {
        eprintln!("audit-log requires --reader and --log");
        return ExitCode::from(1);
    };
    let bytes = match read_file(path) {
        Ok(bytes) => bytes,
        Err(()) => {
            eprintln!("log unreadable");
            return ExitCode::from(1);
        }
    };
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => {
            eprintln!("log invalid");
            return ExitCode::from(1);
        }
    };
    let report = classify_csv(&text, reader);
    match serde_json::to_vec_pretty(&report) {
        Ok(mut bytes) => {
            bytes.push(b'\n');
            let _ = std::io::Write::write_all(&mut std::io::stdout(), &bytes);
            if report.zero_write.proven {
                ExitCode::from(0)
            } else {
                ExitCode::from(2)
            }
        }
        Err(_) => ExitCode::from(1),
    }
}

fn observe_cmd(args: &[String]) -> ExitCode {
    let Some(config_path) = flag(args, "--config") else {
        eprintln!("observe requires --config --password-file --ca-file --log");
        return ExitCode::from(1);
    };
    let Some(password_path) = flag(args, "--password-file") else {
        eprintln!("observe requires --password-file");
        return ExitCode::from(1);
    };
    let Some(ca_path) = flag(args, "--ca-file") else {
        eprintln!("observe requires --ca-file");
        return ExitCode::from(1);
    };
    let Some(log_path) = flag(args, "--log") else {
        eprintln!("observe requires --log");
        return ExitCode::from(1);
    };
    let config_bytes = match read_file(config_path) {
        Ok(bytes) => bytes,
        Err(()) => return ExitCode::from(1),
    };
    let config: ObserveConfig = match source_admission::parse_strict(&config_bytes) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("config invalid: {}", err.fact);
            return ExitCode::from(1);
        }
    };
    if config.protocol != OBSERVE_PROTOCOL {
        eprintln!("config invalid: protocol");
        return ExitCode::from(1);
    }
    let password = match fs::read_to_string(password_path) {
        Ok(value) => Zeroizing::new(value.trim_end_matches(['\n', '\r']).to_string()),
        Err(_) => {
            eprintln!("password file unreadable");
            return ExitCode::from(1);
        }
    };
    let ca = match read_file(ca_path) {
        Ok(bytes) => bytes,
        Err(()) => return ExitCode::from(1),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return ExitCode::from(1),
    };
    let result = runtime.block_on(observe(
        &config,
        &password,
        &ca,
        std::path::Path::new(log_path),
    ));
    drop(password);
    match result {
        Ok(report) => {
            if report.protocol != AUDIT_PROTOCOL {
                return ExitCode::from(1);
            }
            match serde_json::to_vec_pretty(&report) {
                Ok(mut bytes) => {
                    bytes.push(b'\n');
                    let _ = std::io::Write::write_all(&mut std::io::stdout(), &bytes);
                    if report.zero_write.proven {
                        ExitCode::from(0)
                    } else {
                        ExitCode::from(2)
                    }
                }
                Err(_) => ExitCode::from(1),
            }
        }
        Err(err) => {
            eprintln!("observe failed: {}", err.fact);
            ExitCode::from(1)
        }
    }
}

fn finish_decision(decision: &source_admission::Decision) -> ExitCode {
    match decision_json(decision) {
        Ok(bytes) => {
            let _ = std::io::Write::write_all(&mut std::io::stdout(), &bytes);
            if decision.admitted {
                ExitCode::from(0)
            } else {
                ExitCode::from(2)
            }
        }
        Err(_) => ExitCode::from(1),
    }
}
