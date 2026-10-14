//! MongoDB preview command.
//!
//! Exit 3 is a library denial. Exit 2 is a bad argument, an unpinned driver
//! digest, or an admission that was not given the shared-import probe inputs.
//! Exit 5 means `admit` returned true and the pinned pg-builder commands
//! refused the Mongo manifest. `--c1-run` uses exit 0. The customer probe does not.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use mongodb_preview::{
    arrow_matches, authorize_read, carries_client_authority, customer_from_json, decide, frozen_driver,
    gap_document, native_import_document, run_shared_probe, shared_contract_refused, write_probe_inputs,
    CustomerInput,
};

fn main() -> ExitCode {
    if env::args().nth(1).as_deref() == Some("--decode-docs") {
        return decode_docs();
    }
    if env::args().nth(1).as_deref() == Some("--seal-docs") {
        return seal_docs();
    }
    if env::args().nth(1).as_deref() == Some("--mutate-set") {
        return mutate_set();
    }
    if env::args().nth(1).as_deref() == Some("--profile-doc") {
        return profile_doc();
    }
    if env::args().nth(1).as_deref() == Some("--importer-request") {
        return importer_request();
    }
    if env::args().nth(1).as_deref() == Some("--bind-pack") {
        return bind_pack();
    }
    if env::args().nth(1).as_deref() == Some("--emit-native") {
        return emit_native();
    }
    if env::args().nth(1).as_deref() == Some("--plan-sync") {
        return plan_sync();
    }
    if env::args().nth(1).as_deref() == Some("--diff-sync") {
        return diff_sync();
    }
    if env::args().nth(1).as_deref() == Some("--c1-run") {
        return c1_run();
    }
    let mut source_id = None;
    let mut secret_ref = None;
    let mut driver_digest = None;
    let mut permission = None;
    let mut budget_unit = None;
    let mut budget_limit = None;
    let mut out = None;
    let mut gap_out = None;
    let mut pg_builder = None;
    let mut work_dir = None;
    let mut arrow = None;
    let mut census = None;
    let mut customer_file = None;
    let mut read_object = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--source-id" => source_id = args.next(),
            "--secret-ref" => secret_ref = args.next(),
            "--driver-digest" => driver_digest = args.next(),
            "--permission" => permission = args.next(),
            "--budget-unit" => budget_unit = args.next(),
            "--budget-limit" => budget_limit = args.next(),
            "--out" => out = args.next(),
            "--gap-out" => gap_out = args.next(),
            "--pg-builder" => pg_builder = args.next(),
            "--work-dir" => work_dir = args.next(),
            "--arrow" => arrow = args.next(),
            "--census" => census = args.next(),
            "--customer" => customer_file = args.next(),
            "--read-object" => read_object = args.next(),
            _ => return usage(),
        }
    }
    let (
        Some(source_id),
        Some(secret_ref),
        Some(driver_digest),
        Some(permission),
        Some(budget_unit),
        Some(limit_text),
        Some(out),
        Some(gap_out),
    ) = (
        source_id,
        secret_ref,
        driver_digest,
        permission,
        budget_unit,
        budget_limit,
        out,
        gap_out,
    )
    else {
        return usage();
    };
    let Ok(budget_limit) = limit_text.parse::<u64>() else {
        eprintln!("mongodb-preview: budget limit is not an integer");
        return ExitCode::from(2);
    };
    let mut input = match customer_file.as_deref() {
        Some(path) => {
            let text = match fs::read_to_string(path) {
                Ok(text) => text,
                Err(_) => {
                    eprintln!("mongodb-preview: customer file could not be read");
                    return ExitCode::from(2);
                }
            };
            match customer_from_json(&text) {
                Ok(parsed) => parsed,
                Err(fact) => {
                    eprintln!("mongodb-preview: rejected {fact}");
                    return ExitCode::from(2);
                }
            }
        }
        None => {
            eprintln!("mongodb-preview: customer file is required");
            return ExitCode::from(2);
        }
    };
    if input.source_id != source_id
        || input.secret_ref != secret_ref
        || input.driver_digest != driver_digest
        || input.budget_unit != budget_unit
        || input.budget_limit != budget_limit
    {
        eprintln!("mongodb-preview: customer file does not match the flags");
        return ExitCode::from(2);
    }
    input.permission = permission;
    let decision = match decide(&input) {
        Ok(decision) => decision,
        Err(fact) => {
            eprintln!("mongodb-preview: rejected {fact}");
            return ExitCode::from(2);
        }
    };
    let bytes = match source_admission::decision_json(&decision) {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("mongodb-preview: decision could not be encoded");
            return ExitCode::from(1);
        }
    };
    if fs::write(&out, &bytes).is_err() {
        eprintln!("mongodb-preview: decision file could not be written");
        return ExitCode::from(1);
    }
    if !decision.admitted {
        return write_gap(&gap_out, &input, &decision, None, 3);
    }
    if !frozen_driver(&input) {
        eprintln!("mongodb-preview: driver digest is not the frozen crate checksum");
        return write_gap(&gap_out, &input, &decision, None, 2);
    }
    if authorize_read(&input, &input.deny_object, &input.deny_fields).is_ok() {
        eprintln!("mongodb-preview: deny object was approved");
        return ExitCode::from(2);
    }
    if let Some(object) = read_object.as_deref()
        && let Err(fact) = authorize_read(&input, object, &input.approved_fields)
    {
        eprintln!("mongodb-preview: rejected {fact} before read");
        return write_preread(&gap_out, object, fact);
    }
    let (Some(pg_builder), Some(work_dir), Some(arrow), Some(census)) =
        (pg_builder, work_dir, arrow, census)
    else {
        eprintln!("mongodb-preview: admitted input has no shared-import probe");
        return write_gap(&gap_out, &input, &decision, None, 2);
    };
    let arrow_bytes = match fs::read(&arrow) {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("mongodb-preview: arrow file could not be read");
            return ExitCode::from(2);
        }
    };
    if !arrow_matches(&arrow_bytes) {
        eprintln!("mongodb-preview: arrow file is not the frozen 16-shipment batch");
        return write_gap(&gap_out, &input, &decision, None, 2);
    }
    let census_text = match fs::read_to_string(&census) {
        Ok(text) => text,
        Err(_) => {
            eprintln!("mongodb-preview: census file could not be read");
            return ExitCode::from(2);
        }
    };
    let census_value: serde_json::Value = match serde_json::from_str(&census_text) {
        Ok(value) => value,
        Err(_) => {
            eprintln!("mongodb-preview: census file is not JSON");
            return ExitCode::from(2);
        }
    };
    let work = PathBuf::from(&work_dir);
    if write_probe_inputs(
        &work,
        &input,
        &census_value,
        mongodb_preview::FROZEN_ARROW_SHA256,
    )
    .is_err()
    {
        eprintln!("mongodb-preview: probe inputs could not be written");
        return ExitCode::from(1);
    }
    let builder = PathBuf::from(&pg_builder);
    let probe = match run_shared_probe(&builder, &work) {
        Ok(probe) => probe,
        Err(_) => {
            eprintln!("mongodb-preview: pg-builder could not be started");
            return ExitCode::from(1);
        }
    };
    let code = if shared_contract_refused(&probe) { 5 } else { 6 };
    write_gap(&gap_out, &input, &decision, Some(&probe), code)
}

fn write_preread(path: &str, object: &str, reason: &str) -> ExitCode {
    let body = format!(
        "{{\n  \"object\": \"{object}\",\n  \"reason\": \"{reason}\",\n  \"refused\": true,\n  \"find_sent\": false,\n  \"password_read\": false,\n  \"import_started\": false\n}}\n"
    );
    if fs::write(path, body).is_err() {
        eprintln!("mongodb-preview: gap file could not be written");
        return ExitCode::from(1);
    }
    ExitCode::from(2)
}

fn write_gap(
    path: &str,
    input: &CustomerInput,
    decision: &source_admission::Decision,
    probe: Option<&mongodb_preview::ProbeOutput>,
    code: u8,
) -> ExitCode {
    let gap = serde_json::to_vec_pretty(&gap_document(input, decision, probe)).unwrap_or_default();
    if gap.is_empty() || fs::write(path, gap).is_err() {
        eprintln!("mongodb-preview: gap file could not be written");
        return ExitCode::from(1);
    }
    ExitCode::from(code)
}

fn emit_native() -> ExitCode {
    let mut config_digest = None;
    let mut schema_digest = None;
    let mut type_digest = None;
    let mut input_digest = None;
    let mut arrow = None;
    let mut batch_sha = None;
    let mut out = None;
    let mut args = env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config-digest" => config_digest = args.next(),
            "--schema-digest" => schema_digest = args.next(),
            "--type-digest" => type_digest = args.next(),
            "--input-digest" => input_digest = args.next(),
            "--arrow" => arrow = args.next(),
            "--batch-sha" => batch_sha = args.next(),
            "--out" => out = args.next(),
            _ => return usage(),
        }
    }
    let (Some(config_digest), Some(schema_digest), Some(type_digest), Some(input_digest), Some(arrow), Some(batch_sha), Some(out)) =
        (config_digest, schema_digest, type_digest, input_digest, arrow, batch_sha, out)
    else {
        return usage();
    };
    let document = native_import_document(
        &config_digest,
        &schema_digest,
        &type_digest,
        &input_digest,
        &arrow,
        &batch_sha,
    );
    if carries_client_authority(&document) {
        eprintln!("mongodb-preview: native manifest carried client authority");
        return ExitCode::from(2);
    }
    let bytes = serde_json::to_vec_pretty(&document).unwrap_or_default();
    if bytes.is_empty() || fs::write(out, bytes).is_err() {
        eprintln!("mongodb-preview: native manifest could not be written");
        return ExitCode::from(1);
    }
    ExitCode::from(0)
}

fn c1_run() -> ExitCode {
    let mut config = None;
    let mut args = env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => config = args.next(),
            _ => return usage(),
        }
    }
    let Some(config) = config else {
        return usage();
    };
    match mongodb_preview::c1::serve(&config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(detail) => {
            eprintln!("mongodb-preview: c1 {detail}");
            ExitCode::from(1)
        }
    }
}

fn plan_sync() -> ExitCode {
    let mut request_path = None;
    let mut out = None;
    let mut args = env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--request" => request_path = args.next(),
            "--out" => out = args.next(),
            _ => return usage(),
        }
    }
    let (Some(request_path), Some(out)) = (request_path, out) else {
        return usage();
    };
    let text = match fs::read_to_string(request_path) {
        Ok(text) => text,
        Err(_) => {
            eprintln!("mongodb-preview: sync request could not be read");
            return ExitCode::from(2);
        }
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => {
            eprintln!("mongodb-preview: sync request is not JSON");
            return ExitCode::from(2);
        }
    };
    let request = match mongodb_preview::capture::parse_request(&value) {
        Ok(request) => request,
        Err(fact) => {
            eprintln!("mongodb-preview: rejected {fact}");
            return ExitCode::from(2);
        }
    };
    let decision = mongodb_preview::capture::plan(&request);
    let bytes = serde_json::to_vec_pretty(&decision.to_json()).unwrap_or_default();
    if bytes.is_empty() || fs::write(out, bytes).is_err() {
        eprintln!("mongodb-preview: sync decision could not be written");
        return ExitCode::from(1);
    }
    if decision.outcome == "c5_admitted" || decision.outcome == "c1_admitted" {
        ExitCode::from(0)
    } else {
        ExitCode::from(3)
    }
}

fn diff_sync() -> ExitCode {
    let mut request_path = None;
    let mut out = None;
    let mut args = env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--request" => request_path = args.next(),
            "--out" => out = args.next(),
            _ => return usage(),
        }
    }
    let (Some(request_path), Some(out)) = (request_path, out) else {
        return usage();
    };
    let text = match fs::read_to_string(request_path) {
        Ok(text) => text,
        Err(_) => {
            eprintln!("mongodb-preview: diff request could not be read");
            return ExitCode::from(2);
        }
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => {
            eprintln!("mongodb-preview: diff request is not JSON");
            return ExitCode::from(2);
        }
    };
    let before = match snap_rows(&value, "before") {
        Ok(rows) => rows,
        Err(_) => return ExitCode::from(2),
    };
    let after = match snap_rows(&value, "after") {
        Ok(rows) => rows,
        Err(_) => return ExitCode::from(2),
    };
    let tombstones = match value.get("tombstones").and_then(|item| item.as_array()) {
        Some(items) => {
            let mut names = Vec::new();
            for item in items {
                let Some(name) = item.as_str() else {
                    return ExitCode::from(2);
                };
                names.push(name.to_string());
            }
            names
        }
        None => return ExitCode::from(2),
    };
    match mongodb_preview::capture::converge(&before, &after, &tombstones) {
        Ok(caught) => {
            let bytes = serde_json::to_vec_pretty(&mongodb_preview::capture::catch_up_json(&caught)).unwrap_or_default();
            if bytes.is_empty() || fs::write(out, bytes).is_err() {
                return ExitCode::from(1);
            }
            ExitCode::from(0)
        }
        Err(fact) => {
            let body = format!(
                "{{\n  \"protocol\": \"oo.mongodb-preview.sync-diff/v1\",\n  \"converged\": false,\n  \"reason\": \"{fact}\",\n  \"import_invoked\": false,\n  \"generation_created\": false\n}}\n"
            );
            if fs::write(out, body).is_err() {
                return ExitCode::from(1);
            }
            eprintln!("mongodb-preview: diff {fact}");
            ExitCode::from(3)
        }
    }
}

fn snap_rows(value: &serde_json::Value, key: &str) -> Result<Vec<mongodb_preview::capture::SnapRow>, ()> {
    let items = value.get(key).and_then(|item| item.as_array()).ok_or(())?;
    let mut rows = Vec::new();
    for item in items {
        let key = item.get("key").and_then(|item| item.as_str()).ok_or(())?;
        let canonical = item.get("canonical").and_then(|item| item.as_str()).ok_or(())?;
        rows.push(mongodb_preview::capture::SnapRow {
            key: key.to_string(),
            canonical: canonical.to_string(),
        });
    }
    Ok(rows)
}

fn decode_docs() -> ExitCode {
    let mut hex = None;
    let mut out = None;
    let mut args = env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--hex" => hex = args.next(),
            "--out" => out = args.next(),
            _ => return usage(),
        }
    }
    let (Some(hex), Some(out)) = (hex, out) else {
        return usage();
    };
    let text = match fs::read_to_string(hex) {
        Ok(text) => text,
        Err(_) => {
            eprintln!("mongodb-preview: hex file could not be read");
            return ExitCode::from(2);
        }
    };
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let raw = match mongodb_preview::docseal::decode_hex(line) {
            Ok(raw) => raw,
            Err(err) => {
                rows.push(mongodb_preview::docseal::fault_json(&err));
                continue;
            }
        };
        match mongodb_preview::docseal::decode_report(&raw) {
            Ok(mut report) => {
                report["index"] = serde_json::json!(index);
                rows.push(report);
            }
            Err(err) => {
                let mut body = mongodb_preview::docseal::fault_json(&err);
                body["index"] = serde_json::json!(index);
                rows.push(body);
            }
        }
    }
    write_json(&out, &serde_json::json!({
        "protocol": mongodb_preview::docseal::PROTOCOL,
        "source_acceptance": false,
        "rows": rows,
    }))
}

fn seal_docs() -> ExitCode {
    let Some(request) = flag_request() else {
        return usage();
    };
    let (path, out) = request;
    let value = match read_json(&path) {
        Ok(value) => value,
        Err(code) => return code,
    };
    let rows = match value.get("rows").and_then(|item| item.as_array()) {
        Some(rows) => rows,
        None => {
            eprintln!("mongodb-preview: seal rows are absent");
            return ExitCode::from(2);
        }
    };
    let mut sealed = Vec::new();
    for row in rows {
        let name = row.get("name").and_then(|item| item.as_str()).unwrap_or("");
        let raw_hex = row.get("raw_bson_hex").and_then(|item| item.as_str()).unwrap_or("");
        let pg_hex = row.get("pg_jsonb_hex").and_then(|item| item.as_str()).unwrap_or("");
        let pg_text = row.get("pg_jsonb_text").and_then(|item| item.as_str()).unwrap_or("");
        let result = (|| {
            let raw = mongodb_preview::docseal::decode_hex(raw_hex)?;
            let pg = mongodb_preview::docseal::decode_hex(pg_hex)?;
            let envelope = mongodb_preview::docseal::document_envelope(&raw, &pg, pg_text);
            mongodb_preview::docseal::seal_value(&envelope)
        })();
        match result {
            Ok(mut body) => {
                body["name"] = serde_json::json!(name);
                sealed.push(body);
            }
            Err(err) => {
                let mut body = mongodb_preview::docseal::fault_json(&err);
                body["name"] = serde_json::json!(name);
                sealed.push(body);
            }
        }
    }
    write_json(&out, &serde_json::json!({
        "protocol": mongodb_preview::docseal::PROTOCOL,
        "source_acceptance": false,
        "generation_created": false,
        "import_invoked": false,
        "rows": sealed,
    }))
}

fn mutate_set() -> ExitCode {
    let Some((path, out)) = flag_request() else {
        return usage();
    };
    let value = match read_json(&path) {
        Ok(value) => value,
        Err(code) => return code,
    };
    let raw_hex = value.get("raw_bson_hex").and_then(|item| item.as_str()).unwrap_or("");
    let sets = match value.get("sets").and_then(|item| item.as_array()) {
        Some(sets) => sets,
        None => {
            eprintln!("mongodb-preview: mutation sets are absent");
            return ExitCode::from(2);
        }
    };
    let mut raw = match mongodb_preview::docseal::decode_hex(raw_hex) {
        Ok(raw) => raw,
        Err(err) => return write_json(&out, &mongodb_preview::docseal::fault_json(&err)),
    };
    for set in sets {
        let key = set.get("key").and_then(|item| item.as_str()).unwrap_or("");
        let kind = set.get("kind").and_then(|item| item.as_str()).unwrap_or("");
        let text = set.get("value").and_then(|item| item.as_str()).unwrap_or("");
        let next = match kind {
            "string" => mongodb_preview::docseal::set_root_string(&raw, key, text),
            "int32" => match text.parse::<i32>() {
                Ok(number) => mongodb_preview::docseal::set_root_int32(&raw, key, number),
                Err(_) => Err(mongodb_preview::docseal::SealFault {
                    code: "oo.document.bson-jsonb/v1/schema.invalid".to_string(),
                    path: "/set".to_string(),
                    message: "int32 value is not an integer",
                }),
            },
            _ => Err(mongodb_preview::docseal::SealFault {
                code: "oo.document.bson-jsonb/v1/type.unsupported".to_string(),
                path: "/set".to_string(),
                message: "mutation kind is not string or int32",
            }),
        };
        match next {
            Ok(bytes) => raw = bytes,
            Err(err) => return write_json(&out, &mongodb_preview::docseal::fault_json(&err)),
        }
    }
    let report = match mongodb_preview::docseal::decode_report(&raw) {
        Ok(report) => report,
        Err(err) => return write_json(&out, &mongodb_preview::docseal::fault_json(&err)),
    };
    write_json(&out, &serde_json::json!({
        "protocol": mongodb_preview::docseal::PROTOCOL,
        "source_acceptance": false,
        "raw_bson_hex": mongodb_preview::docseal::encode_hex(&raw),
        "decoded": report,
    }))
}

fn profile_doc() -> ExitCode {
    let mut catalog = None;
    let mut envelope = None;
    let mut out = None;
    let mut args = env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--catalog" => catalog = args.next(),
            "--envelope" => envelope = args.next(),
            "--out" => out = args.next(),
            _ => return usage(),
        }
    }
    let (Some(catalog), Some(envelope), Some(out)) = (catalog, envelope, out) else {
        return usage();
    };
    let catalog_text = match fs::read_to_string(&catalog) {
        Ok(text) => text,
        Err(_) => {
            eprintln!("mongodb-preview: catalog file could not be read");
            return ExitCode::from(2);
        }
    };
    let envelope_value = match read_json(&envelope) {
        Ok(value) => value,
        Err(code) => return code,
    };
    match mongodb_preview::docseal::profile_document(&catalog_text, &envelope_value) {
        Ok(body) => write_json(&out, &body),
        Err(err) => {
            let body = mongodb_preview::docseal::fault_json(&err);
            let _ = fs::write(&out, serde_json::to_vec(&body).unwrap_or_default());
            eprintln!("mongodb-preview: profile {}", err.code);
            ExitCode::from(3)
        }
    }
}

fn bind_pack() -> ExitCode {
    let mut seal = None;
    let mut audit = None;
    let mut generation = None;
    let mut parent = None;
    let mut full_snapshot = false;
    let mut out_dir = None;
    let mut arrow_path = None;
    let mut receipt_path = None;
    let mut registry_path = None;
    let mut catalog_path = None;
    let mut driver_path = None;
    let mut unclassified_path = None;
    let mut password_file = None;
    let mut database = None;
    let mut port = None;
    let mut shard = None;
    let mut owner = None;
    let mut args = env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seal" => seal = args.next(),
            "--audit" => audit = args.next(),
            "--generation" => generation = args.next(),
            "--parent" => parent = args.next(),
            "--full-snapshot" => full_snapshot = true,
            "--out-dir" => out_dir = args.next(),
            "--arrow-path" => arrow_path = args.next(),
            "--receipt-path" => receipt_path = args.next(),
            "--registry-path" => registry_path = args.next(),
            "--catalog-path" => catalog_path = args.next(),
            "--driver" => driver_path = args.next(),
            "--unclassified" => unclassified_path = args.next(),
            "--password-file" => password_file = args.next(),
            "--database" => database = args.next(),
            "--port" => port = args.next(),
            "--shard" => shard = args.next(),
            "--owner" => owner = args.next(),
            _ => return usage(),
        }
    }
    let Some(seal_path) = seal else { return usage() };
    let Some(audit_path) = audit else { return usage() };
    let Some(out_dir) = out_dir else { return usage() };
    let Some(arrow_path) = arrow_path else { return usage() };
    let Some(receipt_path) = receipt_path else { return usage() };
    let Some(registry_path) = registry_path else { return usage() };
    let Some(catalog_path) = catalog_path else { return usage() };
    let Some(driver_path) = driver_path else { return usage() };
    let Some(password_file) = password_file else { return usage() };
    let Some(database) = database else { return usage() };
    let Some(shard) = shard else { return usage() };
    let Some(owner) = owner else { return usage() };
    let Some(generation) = generation.and_then(|text| text.parse::<i64>().ok()) else {
        return usage();
    };
    let Some(port) = port.and_then(|text| text.parse::<u16>().ok()) else {
        return usage();
    };
    let parent = match parent {
        None => None,
        Some(text) => match text.parse::<i64>() {
            Ok(value) => Some(value),
            Err(_) => return usage(),
        },
    };
    let seal_bytes = match fs::read(&seal_path) {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("mongodb-preview: seal file could not be read");
            return ExitCode::from(2);
        }
    };
    let seal_value = match serde_json::from_slice::<serde_json::Value>(&seal_bytes) {
        Ok(value) => value,
        Err(_) => {
            eprintln!("mongodb-preview: seal file is not JSON");
            return ExitCode::from(2);
        }
    };
    let audit_bytes = match fs::read(&audit_path) {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("mongodb-preview: audit file could not be read");
            return ExitCode::from(2);
        }
    };
    let driver_bytes = match fs::read(&driver_path) {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("mongodb-preview: driver file could not be read");
            return ExitCode::from(2);
        }
    };
    let unclassified_bytes = match &unclassified_path {
        None => None,
        Some(path) => match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(_) => {
                eprintln!("mongodb-preview: unclassified file could not be read");
                return ExitCode::from(2);
            }
        },
    };
    let unclassified_value = match &unclassified_bytes {
        None => None,
        Some(bytes) => match serde_json::from_slice::<serde_json::Value>(bytes) {
            Ok(value) => Some(value),
            Err(_) => {
                eprintln!("mongodb-preview: unclassified file is not JSON");
                return ExitCode::from(2);
            }
        },
    };
    let packed = match mongodb_preview::bindpack::build_pack(&mongodb_preview::bindpack::PackInput {
        seal: &seal_value,
        seal_bytes: &seal_bytes,
        audit_bytes: &audit_bytes,
        unclassified: unclassified_value.as_ref(),
        unclassified_bytes: unclassified_bytes.as_deref(),
        driver_bytes: &driver_bytes,
        generation,
        parent,
        full_snapshot,
        arrow_path: &arrow_path,
        receipt_path: &receipt_path,
        registry_path: &registry_path,
        catalog_path: &catalog_path,
        audit_path: &audit_path,
        driver_path: &driver_path,
        password_file: &password_file,
        database: &database,
        port,
        shard: &shard,
        owner: &owner,
    }) {
        Ok(packed) => packed,
        Err(err) => {
            eprintln!("mongodb-preview: bind-pack {} {}", err.code, err.message);
            return ExitCode::from(3);
        }
    };
    if fs::create_dir_all(&out_dir).is_err() {
        eprintln!("mongodb-preview: bind-pack directory could not be created");
        return ExitCode::from(1);
    }
    let writes = [
        (format!("{out_dir}/rows.arrow"), packed.arrow.as_slice()),
        (arrow_path.clone(), packed.arrow.as_slice()),
        (format!("{out_dir}/config.json"), packed.config.as_slice()),
        (format!("{out_dir}/hook.json"), packed.hook.as_slice()),
        (format!("{out_dir}/catalog.json"), packed.catalog.as_slice()),
        (catalog_path.clone(), packed.catalog.as_slice()),
        (format!("{out_dir}/registry.json"), packed.registry.as_slice()),
        (registry_path.clone(), packed.registry.as_slice()),
        (format!("{out_dir}/receipt.json"), packed.receipt.as_slice()),
        (receipt_path.clone(), packed.receipt.as_slice()),
        (format!("{out_dir}/request.json"), packed.request.as_slice()),
        (format!("{out_dir}/census.json"), packed.census.as_slice()),
    ];
    for (path, bytes) in writes {
        if fs::write(&path, bytes).is_err() {
            eprintln!("mongodb-preview: bind-pack file could not be written");
            return ExitCode::from(1);
        }
    }
    write_json(&format!("{out_dir}/summary.json"), &packed.summary)
}

fn importer_request() -> ExitCode {
    let Some((path, out)) = flag_request() else {
        return usage();
    };
    let envelope = match read_json(&path) {
        Ok(value) => value,
        Err(code) => return code,
    };
    match mongodb_preview::docseal::importer_request(&envelope) {
        Ok(body) => write_json(&out, &body),
        Err(err) => write_json(&out, &mongodb_preview::docseal::fault_json(&err)),
    }
}

fn flag_request() -> Option<(String, String)> {
    let mut request = None;
    let mut out = None;
    let mut args = env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--request" => request = args.next(),
            "--out" => out = args.next(),
            _ => return None,
        }
    }
    Some((request?, out?))
}

fn read_json(path: &str) -> Result<serde_json::Value, ExitCode> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => {
            eprintln!("mongodb-preview: request file could not be read");
            return Err(ExitCode::from(2));
        }
    };
    serde_json::from_str(&text).map_err(|_| {
        eprintln!("mongodb-preview: request is not JSON");
        ExitCode::from(2)
    })
}

fn write_json(path: &str, value: &serde_json::Value) -> ExitCode {
    let bytes = match serde_json::to_vec(value) {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("mongodb-preview: result could not be encoded");
            return ExitCode::from(1);
        }
    };
    if fs::write(path, bytes).is_err() {
        eprintln!("mongodb-preview: result file could not be written");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn usage() -> ExitCode {
    eprintln!(
        "mongodb-preview --customer FILE --source-id ID --secret-ref secret://NAME --driver-digest HEX --permission ROLE --budget-unit UNIT --budget-limit N --out decision.json --gap-out gap.json [--read-object NAME] [--pg-builder BIN --work-dir DIR --arrow FILE --census FILE]\nmongodb-preview --plan-sync --request FILE --out FILE\nmongodb-preview --diff-sync --request FILE --out FILE\nmongodb-preview --decode-docs --hex FILE --out FILE\nmongodb-preview --seal-docs --request FILE --out FILE\nmongodb-preview --mutate-set --request FILE --out FILE\nmongodb-preview --profile-doc --catalog FILE --envelope FILE --out FILE\nmongodb-preview --importer-request --request FILE --out FILE\nmongodb-preview --bind-pack --seal FILE --audit FILE --generation N --out-dir DIR --arrow-path FILE --receipt-path FILE --registry-path FILE --catalog-path FILE --driver FILE --password-file FILE --database NAME --port N --shard NAME --owner UUID [--parent N] [--full-snapshot] [--unclassified FILE]\nmongodb-preview --c1-run --config FILE"
    );
    ExitCode::from(2)
}
