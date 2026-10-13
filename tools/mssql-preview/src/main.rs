//! Customer SQL Server commands. Exit 0 is a local preflight or codec.
//! Exit 4 is the fixed native-engine refusal. Exit 5 is the pending gate list.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use mssql_preview::{
    driver_report, encode_fixture, exit_class, gates_document, parse_customer, preflight, run_catalog, run_load,
    seal_request, Fail, ObjectName,
};
use serde_json::{json, Value};

fn main() -> ExitCode {
    match execute() {
        Ok(code) => code,
        Err(err) => {
            let payload = json!({"code": err.code, "detail": err.detail});
            eprintln!("{payload}");
            ExitCode::from(exit_class(err.code))
        }
    }
}

fn execute() -> Result<ExitCode, Fail> {
    let args: Vec<String> = env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");
    match command {
        "preflight" => {
            let customer = load_customer(&args)?;
            let ready = preflight(&customer, now_ms(), object_flag(&args)?.as_ref())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "protocol": "oo.mssql-preview.preflight/v1",
                    "socket_opened": ready.socket_opened,
                    "admission_invoked": ready.admission_invoked,
                    "admission_bin_env_present": ready.admission_bin_env_present,
                    "source_acceptance": "pending",
                    "requested_rows": ready.requested_rows,
                    "driver": driver_report()
                })).map_err(|_| Fail::new("output", "preflight json could not be encoded"))?
            );
            Ok(ExitCode::SUCCESS)
        }
        "catalog" => {
            let customer = load_customer(&args)?;
            let out = required(&args, "--out")?;
            let body = run_catalog_sync(&customer, object_flag(&args)?.as_ref())?;
            write_json(&out, &body)?;
            Ok(ExitCode::SUCCESS)
        }
        "load-arrow" => {
            let customer = load_customer(&args)?;
            let out = required(&args, "--out")?;
            let object = object_flag(&args)?.ok_or_else(|| {
                Fail::new("usage", "load-arrow requires --object schema.name")
            })?;
            let package = run_load_sync(&customer, &object)?;
            fs::write(&out, &package.ipc).map_err(|_| Fail::new("output", "arrow file could not be written"))?;
            write_json(&manifest_path(&out), &package.manifest)?;
            Ok(ExitCode::SUCCESS)
        }
        "snapshot-full" | "ct-pull" => {
            let mut customer = load_customer(&args)?;
            if let Some(object)=object_flag(&args)? {
                preflight(&customer,now_ms(),Some(&object))?;
                customer.approved_objects=vec![object];
            }
            let out = required(&args, "--out")?;
            let chunk = flag_value(&args, "--chunk-rows")?.unwrap_or_else(||"1000".into())
                .parse::<u64>().map_err(|_|Fail::new("chunk_size", "invalid chunk size"))?;
            let prior: Option<mssql_preview::Position> = if command == "ct-pull" {
                let path=required(&args,"--position")?;
                Some(serde_json::from_slice(&fs::read(path).map_err(|_|Fail::new("ct_position","position unreadable"))?)
                    .map_err(|_|Fail::new("ct_position","invalid applied position"))?)
            } else { None };
            let runtime=tokio::runtime::Runtime::new().map_err(|_|Fail::new("connect_failed","runtime start"))?;
            let manifest=runtime.block_on(mssql_preview::run_capture(&customer,now_ms(),prior.as_ref(),chunk,std::path::Path::new(&out)))?;
            println!("{}",json!({"protocol":manifest["protocol"],"source_acceptance":"pending","output_written":true,"position_advanced":false}));
            Ok(ExitCode::SUCCESS)
        }
        "audit-export" => {
            let customer=load_customer(&args)?;
            let glob=required(&args,"--audit-file-glob")?;
            let reader=required(&args,"--reader")?;
            let application=required(&args,"--application")?;
            let out=required(&args,"--out")?;
            let runtime=tokio::runtime::Runtime::new().map_err(|_|Fail::new("connect_failed","runtime start"))?;
            let audit=runtime.block_on(mssql_preview::run_audit_export(&customer,now_ms(),&glob,&reader,&application))?;
            write_json(&out,&audit)?;
            Ok(ExitCode::SUCCESS)
        }
        "encode-arrow" => {
            let fixture = required(&args, "--fixture")?;
            let out = required(&args, "--out")?;
            let bytes = fs::read(&fixture).map_err(|_| Fail::new("invalid_fixture", "fixture file could not be read"))?;
            let package = encode_fixture(&bytes)?;
            fs::write(&out, &package.ipc).map_err(|_| Fail::new("output", "arrow file could not be written"))?;
            write_json(&manifest_path(&out), &package.manifest)?;
            Ok(ExitCode::SUCCESS)
        }
        "seal-request" => {
            let customer = load_customer(&args)?;
            let out = required(&args, "--out")?;
            let stop = seal_request(&customer, now_ms())?;
            write_json(&out, &stop.request)?;
            eprintln!(
                "{}",
                json!({"code": stop.code, "detail": stop.detail})
            );
            Ok(ExitCode::from(4))
        }
        "gates" => {
            let document = gates_document(env::var_os("OO_SOURCE_ADMISSION_BIN").is_some());
            println!(
                "{}",
                serde_json::to_string_pretty(&document)
                    .map_err(|_| Fail::new("output", "gates json could not be encoded"))?
            );
            Ok(ExitCode::from(5))
        }
        _ => Err(Fail::new(
            "usage",
            "mssql-preview preflight|catalog|load-arrow|snapshot-full|ct-pull|audit-export|encode-arrow|seal-request|gates",
        )),
    }
}

fn run_catalog_sync(customer: &mssql_preview::Customer, requested: Option<&ObjectName>) -> Result<Value, Fail> {
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|_| Fail::new("connect_failed", "runtime could not start"))?;
    runtime.block_on(run_catalog(customer, now_ms(), requested))
}

fn run_load_sync(customer: &mssql_preview::Customer, object: &ObjectName) -> Result<mssql_preview::ArrowPackage, Fail> {
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|_| Fail::new("connect_failed", "runtime could not start"))?;
    runtime.block_on(run_load(customer, now_ms(), object))
}

fn load_customer(args: &[String]) -> Result<mssql_preview::Customer, Fail> {
    let path = required(args, "--config")?;
    let bytes = fs::read(&path).map_err(|_| Fail::new("invalid_config", "customer config could not be read"))?;
    parse_customer(&bytes)
}

fn required(args: &[String], flag: &str) -> Result<String, Fail> {
    flag_value(args, flag)?.ok_or_else(|| Fail::new("usage", flag))
}

fn flag_value(args: &[String], flag: &str) -> Result<Option<String>, Fail> {
    let mut found = None;
    let mut index = 1;
    while index < args.len() {
        if args[index] == flag {
            let value = args.get(index + 1).ok_or_else(|| Fail::new("usage", flag))?;
            if value.starts_with("--") {
                return Err(Fail::new("usage", flag));
            }
            found = Some(value.clone());
            index += 2;
            continue;
        }
        index += 1;
    }
    Ok(found)
}

fn object_flag(args: &[String]) -> Result<Option<ObjectName>, Fail> {
    let Some(value) = flag_value(args, "--object")? else {
        return Ok(None);
    };
    let Some((schema, name)) = value.split_once('.') else {
        return Err(Fail::new("identifier", "object must be schema.name"));
    };
    if name.contains('.') || schema.is_empty() || name.is_empty() {
        return Err(Fail::new("identifier", "object must be schema.name"));
    }
    Ok(Some(ObjectName {
        schema: schema.to_string(),
        name: name.to_string(),
    }))
}

fn write_json(path: &str, value: &Value) -> Result<(), Fail> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| Fail::new("output", "json could not be encoded"))?;
    fs::write(path, bytes).map_err(|_| Fail::new("output", "output file could not be written"))
}

fn manifest_path(arrow_path: &str) -> String {
    format!("{arrow_path}.manifest.json")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
