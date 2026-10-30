//! `oo-source-admission` reads files. Passwords stay in files, not argv.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use source_admission::{
    AUDIT_PROTOCOL, CapturedInput, CapturedInputV2, InputFault, OBSERVE_PROTOCOL, ObserveConfig,
    OutputPaths, PartSource, SourceConfig, TargetReport, adapt, admit, admit_pg_budget,
    classify_csv, decide_captured, decide_captured_v2, decision_json, observe, recipe_json,
    registration_recipe, seal_captured, seal_captured_v2, witness_is_v2, witness_paths,
    witness_paths_v2,
};
use zeroize::Zeroizing;

#[path = "../../../crates/oo-pipeline/src/pg_current_capture.rs"]
mod pg_current_capture;

/// Largest part file admission reads. This is `native_arrow::MAX_ARROW`.
const PART_READ_LIMIT: u64 = 32 * 1024 * 1024;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(cmd) = args.next() else {
        eprintln!(
            "usage: oo-source-admission validate|seal-native|adapt-pg|admit-pg-budget|audit-log|observe|registration-recipe"
        );
        return ExitCode::from(1);
    };
    match cmd.as_str() {
        "validate-pg-current" if args.next().is_none() => {
            let result = (|| {
                let pin: pg_current_capture::Artifact =
                    serde_json::from_reader(std::io::stdin()).map_err(|_| "pg_request")?;
                pg_current_capture::decision(&pin)
            })();
            match result {
                Ok(value) => { println!("{value}"); ExitCode::SUCCESS }
                Err(reason) => {
                    println!("{}", serde_json::json!({"protocol":"oo.source-admission.pg-current/v1","admitted":false,"reason":reason}));
                    ExitCode::from(1)
                }
            }
        }
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
    if witness_file_is_v2(witness_path) {
        return validate_captured_v2(report, config, report_bytes, config_bytes, witness_path);
    }
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

fn witness_file_is_v2(witness_path: &str) -> bool {
    fs::read(witness_path).is_ok_and(|bytes| witness_is_v2(&bytes))
}

/// Reads the parts of a v2 admission from disk. Each read is bounded, so a part
/// over 32 MiB is refused and never held in memory.
struct FsParts;

impl PartSource for FsParts {
    fn read(&mut self, dir: &Path, name: &str) -> Result<Vec<u8>, InputFault> {
        let file = fs::File::open(dir.join(name)).map_err(|_| InputFault {
            fact: "part unreadable".to_string(),
        })?;
        let mut bytes = Vec::new();
        file.take(PART_READ_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| InputFault {
                fact: "part unreadable".to_string(),
            })?;
        if bytes.len() as u64 > PART_READ_LIMIT {
            return Err(InputFault {
                fact: "part too large".to_string(),
            });
        }
        Ok(bytes)
    }
}

struct LoadedV2 {
    witness: Vec<u8>,
    capturer: Vec<u8>,
    table_manifest: Vec<u8>,
    cdc_batches: Vec<u8>,
    driver: Vec<u8>,
    schema_preimage: Vec<u8>,
    type_preimage: Vec<u8>,
}

fn load_captured_v2(witness_path: &str) -> Result<LoadedV2, String> {
    if !absolute_cli(witness_path) {
        return Err("path".to_string());
    }
    let witness = fs::read(witness_path).map_err(|_| "witness unreadable".to_string())?;
    let files = witness_paths_v2(&witness).map_err(|err| err.fact)?;
    let read = |path: &str, fact: &str| fs::read(path).map_err(|_| fact.to_string());
    Ok(LoadedV2 {
        capturer: read(&files.capturer_path, "capturer unreadable")?,
        table_manifest: read(&files.table_manifest_path, "manifest unreadable")?,
        cdc_batches: read(&files.cdc_batches_path, "cdc batches unreadable")?,
        driver: read(&files.driver_path, "driver unreadable")?,
        schema_preimage: read(&files.schema_preimage_path, "schema preimage unreadable")?,
        type_preimage: read(&files.type_preimage_path, "type preimage unreadable")?,
        witness,
    })
}

fn validate_captured_v2(
    report: &TargetReport,
    config: &SourceConfig,
    report_bytes: &[u8],
    config_bytes: &[u8],
    witness_path: &str,
) -> ExitCode {
    let loaded = match load_captured_v2(witness_path) {
        Ok(loaded) => loaded,
        Err(fact) => {
            eprintln!("witness invalid: {fact}");
            return ExitCode::from(1);
        }
    };
    let input = CapturedInputV2 {
        witness: &loaded.witness,
        capturer: &loaded.capturer,
        table_manifest: &loaded.table_manifest,
        cdc_batches: &loaded.cdc_batches,
        driver: &loaded.driver,
        schema_preimage: &loaded.schema_preimage,
        type_preimage: &loaded.type_preimage,
        report: report_bytes,
        config: config_bytes,
    };
    match decide_captured_v2(report, config, &input, &mut FsParts) {
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
    let capturer = fs::read(&files.capturer_path).map_err(|_| "capturer unreadable".to_string())?;
    let document: serde_json::Value = serde_json::from_slice(&capturer).map_err(|_| "capturer invalid".to_string())?;
    if let Some(scope) = document.get("multipart_scope") {
        source_admission::native_c5_multipart::Manifest::parse(scope).and_then(|manifest|
            manifest.verify_files(Path::new(&files.capturer_path).parent().ok_or("multipart_path")?))
            .map_err(str::to_string)?;
    }
    let witness_json:serde_json::Value=serde_json::from_slice(&witness).map_err(|_|"witness invalid".to_string())?;
    if witness_json["multipart_anchor"]==true && fs::metadata(&files.arrow_path).map_err(|_|"arrow unreadable".to_string())?.len()>32*1024*1024 {return Err("multipart anchor size".to_string());}
    Ok(Loaded {
        capturer,
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
    if witness_file_is_v2(witness_path) {
        return seal_native_v2(
            &final_dir,
            &report_path,
            &config_path,
            &report,
            &config,
            &report_bytes,
            &config_bytes,
            witness_path,
        );
    }
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

#[allow(clippy::too_many_arguments)]
fn seal_native_v2(
    final_dir: &Path,
    report_path: &str,
    config_path: &str,
    report: &TargetReport,
    config: &SourceConfig,
    report_bytes: &[u8],
    config_bytes: &[u8],
    witness_path: &str,
) -> ExitCode {
    let (Some(catalog_path), Some(receipt_path)) = (
        path_string(&final_dir.join("catalog.json")),
        path_string(&final_dir.join("admission-receipt.json")),
    ) else {
        eprintln!("path");
        return ExitCode::from(1);
    };
    let loaded = match load_captured_v2(witness_path) {
        Ok(loaded) => loaded,
        Err(fact) => {
            eprintln!("witness invalid: {fact}");
            return ExitCode::from(1);
        }
    };
    let input = CapturedInputV2 {
        witness: &loaded.witness,
        capturer: &loaded.capturer,
        table_manifest: &loaded.table_manifest,
        cdc_batches: &loaded.cdc_batches,
        driver: &loaded.driver,
        schema_preimage: &loaded.schema_preimage,
        type_preimage: &loaded.type_preimage,
        report: report_bytes,
        config: config_bytes,
    };
    let paths = OutputPaths {
        report_path,
        config_path,
        catalog_path: &catalog_path,
        receipt_path: &receipt_path,
    };
    let sealed = match seal_captured_v2(report, config, &input, &mut FsParts, &paths) {
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
    if !publish(final_dir, &bundle) {
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
