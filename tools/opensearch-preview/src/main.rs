//! `opensearch-preview decide` writes the frozen admission result.

#![allow(clippy::uninlined_format_args, clippy::redundant_closure_for_method_calls)]

use std::env;
use std::fs;
use std::process::ExitCode;

use opensearch_preview::{admission_documents, customer_document, decide, gap_document, precheck, CustomerInput};
use serde_json::Value;

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("opensearch-preview: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<u8, String> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or("usage: opensearch-preview decide --request FILE --out DIR")?;
    if command != "decide" {
        return Err(format!("unknown command {command}"));
    }
    let mut request = None;
    let mut out = None;
    let mut digest = None;
    while let Some(key) = args.next() {
        let value = args.next().ok_or_else(|| format!("missing {key}"))?;
        match key.as_str() {
            "--request" => request = Some(value),
            "--out" => out = Some(value),
            "--driver-digest" => digest = Some(value),
            other => return Err(format!("unknown arg {other}")),
        }
    }
    let request_path = request.ok_or("missing --request")?;
    let out = out.ok_or("missing --out")?;
    let text = fs::read_to_string(&request_path).map_err(|error| error.to_string())?;
    if text.contains("password=") || text.contains("BEGIN PRIVATE") {
        return Err("request contains secret material".into());
    }
    let request: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if request.get("observed_actions").is_some() {
        return Err("observed_actions is not an admission input".into());
    }
    let input = customer_input(&request)?;
    fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    if let Err(error) = precheck(&input) {
        let refusal = serde_json::json!({
            "refused": true,
            "reason": error,
            "requested_index": input.requested_index,
            "approved_index": input.approved_index,
            "requested_fields": input.requested_fields,
        });
        write_json(&format!("{out}/refusal.json"), &refusal)?;
        return Ok(4);
    }
    let digest = digest.unwrap_or_else(|| "0".repeat(64));
    let decision = decide(&digest, &input)?;
    let (report, config) = admission_documents(&digest, &input)?;
    let gap = gap_document(&decision);
    write_json(&format!("{out}/decision.json"), &serde_json::to_value(&decision).map_err(|error| error.to_string())?)?;
    write_json(&format!("{out}/gap.json"), &gap)?;
    write_json(&format!("{out}/report.json"), &report)?;
    write_json(&format!("{out}/config.json"), &config)?;
    write_json(&format!("{out}/customer.json"), &customer_document(&input))?;
    Ok(if decision.admitted { 0 } else { 4 })
}

fn customer_input(request: &Value) -> Result<CustomerInput, String> {
    Ok(CustomerInput {
        source_id: required_str(request, "source_id")?,
        secret_ref: required_str(request, "secret_ref")?,
        approved_index: required_str(request, "approved_index")?,
        approved_fields: required_strings(request, "approved_fields")?,
        requested_index: required_str(request, "index")?,
        requested_fields: required_strings(request, "requested_fields")?,
        budget_unit: required_str(request, "budget_unit")?,
        budget_hard_limit: required_u64(request, "budget_hard_limit")?,
        target_id: required_str(request, "target_id")?,
        role: required_str(request, "role")?,
        relation: required_str(request, "relation")?,
        measured_rows: required_u64(request, "measured_rows")?,
        search_status: required_status(request, "search_status")?,
        mget_status: required_status(request, "mget_status")?,
    })
}

fn required_str(request: &Value, key: &str) -> Result<String, String> {
    request.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("missing {key}"))
}

fn required_u64(request: &Value, key: &str) -> Result<u64, String> {
    request.get(key).and_then(Value::as_u64).ok_or_else(|| format!("missing {key}"))
}

fn required_status(request: &Value, key: &str) -> Result<u16, String> {
    let value = required_u64(request, key)?;
    u16::try_from(value).map_err(|_| format!("{key} is not a status"))
}

fn required_strings(request: &Value, key: &str) -> Result<Vec<String>, String> {
    let items = request.get(key).and_then(Value::as_array).ok_or_else(|| format!("missing {key}"))?;
    let mut out = Vec::new();
    for item in items {
        let text = item.as_str().ok_or_else(|| format!("{key} entry"))?;
        out.push(text.to_string());
    }
    Ok(out)
}

fn write_json(path: &str, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, text + "\n").map_err(|error| error.to_string())
}
