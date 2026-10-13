//! Customer loop. Secrets stay in the files named by the config.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::{json, Value};

use crate::admission;
use crate::catalog::{self, NAMESPACE, SOURCE_ID};
use crate::import::{self, PgTarget};
use crate::names::sha256_hex;
use crate::CONFIG_PROTOCOL;

pub fn run(config_path: &Path, out_dir: &Path) -> Result<u8, String> {
    fs::create_dir_all(out_dir).map_err(|err| err.to_string())?;
    let config = read_json(config_path)?;
    require(&config, "protocol", CONFIG_PROTOCOL)?;
    let image = text(&config, "image_digest")?;
    let secret_ref = text(&config, "secret_ref")?;
    let row_cap = config.get("row_cap").and_then(Value::as_u64).ok_or("row_cap")?;
    let role = text(&config, "role")?;
    let login = text(&config, "login")?;
    let dataset = text(&config, "dataset")?;
    let reviewer = text(&config, "reviewer")?;
    let reviewed_at = text(&config, "reviewed_at")?;
    let source_detached = config.get("source_detached").and_then(Value::as_bool).ok_or("source_detached")?;
    let probe = path_of(&config, "probe_bin")?;
    let driver_digest = sha256_hex(&fs::read(&probe).map_err(|err| err.to_string())?);
    let pg = object(&config, "pg")?;
    let model = object(&config, "model")?;
    let bins = object(&config, "bins")?;
    let target = PgTarget {
        host: text(pg, "host")?.to_string(),
        port: pg.get("port").and_then(Value::as_u64).ok_or("port")? as u16,
        database: text(pg, "database")?.to_string(),
        user: text(pg, "admin_user")?.to_string(),
        password_file: text(pg, "admin_password_file")?.to_string(),
        ca_file: text(pg, "ca_file")?.to_string(),
    };
    let mut steps = Vec::new();
    let admission_step = run_admission(out_dir, bins, SOURCE_ID, secret_ref, &driver_digest, row_cap)?;
    let admission_ok = admission_step.get("ok").and_then(Value::as_bool) == Some(true);
    steps.push(admission_step);
    let decoded_dir = out_dir.join("decoded");
    let decode_step = run_decode(bins, &config, &decoded_dir)?;
    let decode_ok = decode_step.get("ok").and_then(Value::as_bool) == Some(true);
    steps.push(decode_step);
    let loaded = if decode_ok { load_decoded(&decoded_dir)? } else { Vec::new() };
    let config_digest = sha256_hex(catalog::config_preimage(image, row_cap).as_bytes());
    let type_digest = sha256_hex(catalog::type_preimage().as_bytes());
    let schema_digest = sha256_hex(catalog::schema_text().as_bytes());
    let hook = catalog::hook_digest();
    let input_digest = if loaded.is_empty() {
        sha256_hex(b"starrocks-preview/v1 input absent\n")
    } else {
        sha256_hex(&decoded_bytes(&decoded_dir)?)
    };
    let (namespace_physical, namespace_digest, mapped) = catalog::map_tables(&config_digest);
    let mut import_error = None;
    let mut imported = None;
    if decode_ok {
        let init = call_init(out_dir, bins, &target)?;
        steps.push(init.clone());
        if init.get("ok").and_then(Value::as_bool) == Some(true) {
            let sealed = call_install_sealed(out_dir, bins, &target)?;
            steps.push(sealed.clone());
            if sealed.get("ok").and_then(Value::as_bool) == Some(true) {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|err| err.to_string())?;
                match runtime.block_on(import::import_tables(import::ImportRequest {
                    target: &target,
                    config_digest: &config_digest,
                    type_digest: &type_digest,
                    input_digest: &input_digest,
                    schema_digest: &schema_digest,
                    hook_digest: &hook,
                    namespace_physical: &namespace_physical,
                    namespace_digest: &namespace_digest,
                    mapped: &mapped,
                    decoded: &loaded,
                    login,
                    login_password_file: text(pg, "login_password_file")?,
                })) {
                    Ok(output) => imported = Some(output),
                    Err(err) => import_error = Some(err),
                }
            }
        }
    }
    if let Some(err) = &import_error {
        steps.push(json!({"name": "sealed_import", "ok": false, "error": err}));
    }
    let mut catalog_path = None;
    if let Some(output) = &imported {
        let document = catalog::catalog_document(catalog::CatalogParts {
            config_digest: &config_digest,
            input_digest: &input_digest,
            type_digest: &type_digest,
            schema_digest: &schema_digest,
            run_id: &output.run_id,
            target_generation: output.target_generation,
            mapped: &mapped,
            namespace_physical: &namespace_physical,
            accepted: &output.accepted,
            canonical: &output.canonical,
            quality_rows: &output.quality_rows,
        });
        let path = out_dir.join("sealed-catalog.json");
        write_json(&path, &document)?;
        fs::write(out_dir.join("sealed-identity.json"), serde_json::to_vec_pretty(&output.identity).map_err(|err| err.to_string())?)
            .map_err(|err| err.to_string())?;
        catalog_path = Some(path);
        steps.push(json!({
            "name": "sealed_import",
            "ok": output.identity.get("state").and_then(Value::as_str) == Some("awaiting_publish"),
            "run_id": output.run_id,
            "target_generation": output.target_generation,
            "accepted": output.accepted
        }));
        let refusal = call_catalog_import(out_dir, bins, &target, &document)?;
        steps.push(refusal);
    }
    let mut builder_ok = false;
    if let Some(path) = &catalog_path {
        let settings = settings_json();
        let visibility = visibility_json(role);
        write_json(&out_dir.join("settings.json"), &settings)?;
        write_json(&out_dir.join("visibility.json"), &visibility)?;
        let bearer = text(model, "bearer_file")?;
        let origin = text(model, "origin")?;
        let login_password = text(pg, "login_password_file")?;
        let unknown = call_builder(BuilderCall {
            bins,
            catalog: path,
            target: &target,
            login,
            login_password,
            origin,
            bearer,
            price: r#"{"status":"unknown"}"#,
            deadline_ms: 30_000,
            reviewer,
            reviewed_at,
            dataset,
            role,
            out_dir: &out_dir.join("builder-unknown"),
            cache: &out_dir.join("cache"),
        })?;
        steps.push(unknown);
        let declared = call_builder(BuilderCall {
            bins,
            catalog: path,
            target: &target,
            login,
            login_password,
            origin,
            bearer,
            price: r#"{"status":"declared_non_billing","currency":"USD","per_total_token":"0"}"#,
            deadline_ms: 30_000,
            reviewer,
            reviewed_at,
            dataset,
            role,
            out_dir: &out_dir.join("builder"),
            cache: &out_dir.join("cache"),
        })?;
        let declared_ok = declared.get("ok").and_then(Value::as_bool) == Some(true);
        let declared_timed_out = declared.get("reason").and_then(Value::as_str) == Some("timeout");
        builder_ok = declared_ok;
        steps.push(declared);
        // The 30000 ms budget still ends that call. A cold completion can exceed it
        // and the next identical call can finish inside the same budget. The timeout
        // stays in the receipt. The retry writes the pack the operator reads.
        if !declared_ok && declared_timed_out {
            let mut retry = call_builder(BuilderCall {
                bins,
                catalog: path,
                target: &target,
                login,
                login_password,
                origin,
                bearer,
                price: r#"{"status":"declared_non_billing","currency":"USD","per_total_token":"0"}"#,
                deadline_ms: 30_000,
                reviewer,
                reviewed_at,
                dataset,
                role,
                out_dir: &out_dir.join("builder"),
                cache: &out_dir.join("cache"),
            })?;
            if let Some(map) = retry.as_object_mut() {
                map.insert("name".to_string(), json!("builder-retry"));
            }
            builder_ok = retry.get("ok").and_then(Value::as_bool) == Some(true);
            steps.push(retry);
        }
        let cached = call_builder(BuilderCall {
            bins,
            catalog: path,
            target: &target,
            login,
            login_password,
            origin,
            bearer,
            price: r#"{"status":"declared_non_billing","currency":"USD","per_total_token":"0"}"#,
            deadline_ms: 30_000,
            reviewer,
            reviewed_at,
            dataset,
            role,
            out_dir: &out_dir.join("builder-cache"),
            cache: &out_dir.join("cache"),
        })?;
        steps.push(cached);
        let bad = text(model, "bad_bearer_file")?;
        let unauthorized = call_builder(BuilderCall {
            bins,
            catalog: path,
            target: &target,
            login,
            login_password,
            origin,
            bearer: bad,
            price: r#"{"status":"declared_non_billing","currency":"USD","per_total_token":"0"}"#,
            deadline_ms: 30_000,
            reviewer,
            reviewed_at,
            dataset,
            role,
            out_dir: &out_dir.join("builder-unauthorized"),
            cache: &out_dir.join("cache-unauthorized"),
        })?;
        steps.push(unauthorized);
        let timed = call_builder(BuilderCall {
            bins,
            catalog: path,
            target: &target,
            login,
            login_password,
            origin,
            bearer,
            price: r#"{"status":"declared_non_billing","currency":"USD","per_total_token":"0"}"#,
            deadline_ms: 1,
            reviewer,
            reviewed_at,
            dataset,
            role,
            out_dir: &out_dir.join("builder-timeout"),
            cache: &out_dir.join("cache-timeout"),
        })?;
        steps.push(timed);
    }
    if builder_ok {
        let authority = out_dir.join("builder").join("authority.json");
        let mut perms = fs::metadata(&authority).map_err(|err| err.to_string())?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&authority, perms).map_err(|err| err.to_string())?;
        let acl = call_acl(out_dir, bins, &target, login)?;
        steps.push(acl.clone());
        if acl.get("ok").and_then(Value::as_bool) == Some(true) {
            let described = call_operator(bins, &authority, &["describe", "--authority"], None, &out_dir.join("describe"))?;
            steps.push(described);
            let query_path = out_dir.join("query.json");
            write_json(
                &query_path,
                &json!({
                    "protocol": "oo.operator-preview.query/v1",
                    "sql": format!("SELECT account_id, balance, nickname, opened FROM {dataset} ORDER BY account_id"),
                    "max_rows": 100,
                    "max_result_bytes": 65536,
                    "wall_clock_ms": 10000
                }),
            )?;
            let queried = call_operator(
                bins,
                &authority,
                &["query", "--authority"],
                Some(&query_path),
                &out_dir.join("query"),
            )?;
            let balance = pk_balance(&decoded_dir).unwrap_or_default();
            let stdout = fs::read(out_dir.join("query").join("stdout")).unwrap_or_default();
            let exact = !balance.is_empty() && stdout.windows(balance.len()).any(|window| window == balance.as_bytes());
            steps.push(json!({
                "name": "operator_query",
                "ok": queried.get("exit") == Some(&json!(0)) && exact,
                "exit": queried.get("exit").cloned().unwrap_or(Value::Null),
                "balance_present": exact
            }));
            let cancel = cancel_query(bins, &authority, &target, &mapped, &query_path, out_dir)?;
            steps.push(cancel);
            let repeated = call_operator(
                bins,
                &authority,
                &["query", "--authority"],
                Some(&query_path),
                &out_dir.join("query-repeat"),
            )?;
            let repeat_out = fs::read(out_dir.join("query-repeat").join("stdout")).unwrap_or_default();
            let repeat_exact = !balance.is_empty() && repeat_out.windows(balance.len()).any(|window| window == balance.as_bytes());
            steps.push(json!({
                "name": "source_detached_repeat",
                "ok": source_detached && repeated.get("exit") == Some(&json!(0)) && repeat_exact,
                "source_detached": source_detached,
                "exit": repeated.get("exit").cloned().unwrap_or(Value::Null)
            }));
        }
    }
    let ok = steps.iter().all(|step| step.get("ok").and_then(Value::as_bool) == Some(true)) && admission_ok;
    let receipt = json!({
        "protocol": "oo.starrocks-preview.receipt/v1",
        "source_id": SOURCE_ID,
        "engine": "starrocks",
        "admitted": false,
        "source_detached": source_detached,
        "config_digest": config_digest,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "input_digest": input_digest,
        "driver_digest": driver_digest,
        "eventual_consistency": "S0 primary and unique visibility stays a source caveat; one immediate observation does not remove it",
        "steps": steps,
        "ok": ok
    });
    write_json(&out_dir.join("receipt.json"), &receipt)?;
    Ok(if ok { 0 } else { 1 })
}

fn run_admission(
    out_dir: &Path,
    bins: &Value,
    source_id: &str,
    secret_ref: &str,
    driver_digest: &str,
    row_cap: u64,
) -> Result<Value, String> {
    let (report, config) = admission::documents(source_id, secret_ref, driver_digest, row_cap);
    let report_path = out_dir.join("admission-report.json");
    let config_path = out_dir.join("admission-config.json");
    write_json(&report_path, &report)?;
    write_json(&config_path, &config)?;
    let bin = text(bins, "admission")?;
    let output = captured(
        Command::new(bin).args([
            "validate",
            "--report",
            report_path.to_str().ok_or("path")?,
            "--config",
            config_path.to_str().ok_or("path")?,
        ]),
    )?;
    let decision: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    let blocked = decision
        .get("blocks")
        .and_then(Value::as_array)
        .map(|items| items.iter().any(|item| item.get("code").and_then(Value::as_str) == Some("COMPAT_MODE_UNKNOWN")))
        .unwrap_or(false);
    let admitted = decision.get("admitted").and_then(Value::as_bool) == Some(true);
    save_step(out_dir, "admission", &output)?;
    Ok(json!({
        "name": "admission",
        "ok": !admitted && blocked && decision.get("zero_write").and_then(|item| item.get("proven")).and_then(Value::as_bool) == Some(false),
        "exit": output.status,
        "admitted": admitted,
        "compat_block": blocked
    }))
}

fn run_decode(bins: &Value, config: &Value, decoded_dir: &Path) -> Result<Value, String> {
    let arrow = path_of(config, "arrow_dir")?;
    let mut command = Command::new(text(bins, "decode")?);
    command.arg("--out").arg(decoded_dir);
    let mut count = 0;
    for entry in fs::read_dir(&arrow).map_err(|err| err.to_string())? {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("arrow") {
            command.arg(path);
            count += 1;
        }
    }
    if count == 0 {
        return Ok(json!({"name": "decode", "ok": false, "error": "no arrow files"}));
    }
    let output = captured(&mut command)?;
    Ok(json!({"name": "decode", "ok": output.status == 0 && count >= 5, "exit": output.status, "files": count}))
}

fn load_decoded(dir: &Path) -> Result<Vec<Vec<Vec<Option<String>>>>, String> {
    let mut loaded = Vec::new();
    for table in catalog::tables() {
        let path = dir.join(format!("{}.json", table.name));
        let value = read_json(&path)?;
        if value.get("protocol").and_then(Value::as_str) != Some("oo.source.starrocks.ipc-decode/v1") {
            return Err(format!("{} protocol", table.name));
        }
        let rows = value.get("rows").and_then(Value::as_array).ok_or("rows")?;
        let mut table_rows = Vec::new();
        for row in rows {
            let object = row.as_object().ok_or("row")?;
            let mut cells = Vec::new();
            for column in table.columns {
                let cell = object.get(column.name).ok_or(column.name)?;
                cells.push(cell_text(cell)?);
            }
            table_rows.push(cells);
        }
        if table_rows.is_empty() {
            return Err(format!("{} empty", table.name));
        }
        loaded.push(table_rows);
    }
    Ok(loaded)
}

fn cell_text(value: &Value) -> Result<Option<String>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        Value::Number(number) => {
            if number.is_f64() && number.as_i64().is_none() && number.as_u64().is_none() {
                return Err("float cell".to_string());
            }
            Ok(Some(number.to_string()))
        }
        _ => Err("cell".to_string()),
    }
}

fn decoded_bytes(dir: &Path) -> Result<Vec<u8>, String> {
    let mut names = fs::read_dir(dir).map_err(|err| err.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|err| err.to_string())?;
    names.sort_by_key(|entry| entry.file_name());
    let mut bytes = Vec::new();
    for entry in names {
        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("json") {
            bytes.extend(fs::read(entry.path()).map_err(|err| err.to_string())?);
        }
    }
    Ok(bytes)
}

fn call_init(out_dir: &Path, bins: &Value, target: &PgTarget) -> Result<Value, String> {
    let body = json!({
        "protocol": "oo.pipeline.init-pg/v1",
        "target": {
            "host": target.host,
            "port": target.port,
            "database": target.database,
            "administrator": target.user,
            "password_file": target.password_file,
            "ca_file": target.ca_file
        }
    });
    let mut command = Command::new(text(bins, "init_pg")?);
    let output = captured_stdin(&mut command, &serde_json::to_vec(&body).map_err(|err| err.to_string())?)?;
    save_step(out_dir, "init-pg", &output)?;
    Ok(json!({"name": "init_pg", "ok": output.status == 0, "exit": output.status}))
}

fn call_install_sealed(out_dir: &Path, bins: &Value, target: &PgTarget) -> Result<Value, String> {
    let body = json!({
        "protocol": "oo.pipeline.install-sealed-catalog/v1",
        "target": {
            "host": target.host,
            "port": target.port,
            "database": target.database,
            "administrator": target.user,
            "password_file": target.password_file,
            "ca_file": target.ca_file
        }
    });
    let mut command = Command::new(text(bins, "pipeline")?);
    command.arg("install-sealed-catalog");
    let output = captured_stdin(&mut command, &serde_json::to_vec(&body).map_err(|err| err.to_string())?)?;
    save_step(out_dir, "install-sealed-catalog", &output)?;
    Ok(json!({"name": "install_sealed_catalog", "ok": output.status == 0, "exit": output.status}))
}

fn call_catalog_import(out_dir: &Path, bins: &Value, target: &PgTarget, catalog: &Value) -> Result<Value, String> {
    let tables = catalog.get("tables").and_then(Value::as_array).cloned().unwrap_or_default();
    let manifest_tables = tables
        .iter()
        .map(|table| {
            json!({
                "logical_schema": table.get("logical_schema").cloned().unwrap_or(Value::Null),
                "logical_table": table.get("logical_table").cloned().unwrap_or(Value::Null),
                "accepted_rows": table.get("accepted_rows").cloned().unwrap_or(Value::Null),
                "canonical_digest": table.get("canonical_digest").cloned().unwrap_or(Value::Null),
                "columns": table.get("columns").cloned().unwrap_or(Value::Null),
                "rejects": []
            })
        })
        .collect::<Vec<_>>();
    let body = json!({
        "protocol": "oo.pipeline.catalog-import/v1",
        "source_id": SOURCE_ID,
        "config_digest": catalog.get("config_digest").cloned().unwrap_or(Value::Null),
        "source_generation": 1,
        "manifest": {
            "protocol": "oo.mysql-preview.manifest/v1",
            "engine": "starrocks",
            "source_id": SOURCE_ID,
            "config_digest": catalog.get("config_digest").cloned().unwrap_or(Value::Null),
            "source_generation": 1,
            "target_generation": catalog.get("target_generation").cloned().unwrap_or(json!(1)),
            "run_id": catalog.get("run_id").cloned().unwrap_or(Value::Null),
            "schema_digest": catalog.get("schema_digest").cloned().unwrap_or(Value::Null),
            "type_digest": catalog.get("type_digest").cloned().unwrap_or(Value::Null),
            "input_digest": catalog.get("input_digest").cloned().unwrap_or(Value::Null),
            "hook_digest": catalog.get("tool_hook_digest").cloned().unwrap_or(Value::Null),
            "payload_includes_reject_lines": false,
            "tables": manifest_tables
        },
        "target": {
            "host": target.host,
            "port": target.port,
            "database": target.database,
            "runtime_principal": "oo_pipeline_runtime",
            "password_file": target.password_file,
            "ca_file": target.ca_file
        }
    });
    // The manifest column objects include stored_physical. The official parser
    // denies that field before the engine check if it is strict. Strip storage
    // names so the refusal that remains is the engine token.
    let mut command = Command::new(text(bins, "pipeline")?);
    command.arg("catalog-import");
    let output = captured_stdin(&mut command, &serde_json::to_vec(&strip_storage(&body)).map_err(|err| err.to_string())?)?;
    save_step(out_dir, "catalog-import", &output)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let refused = stderr.contains("source engine");
    Ok(json!({
        "name": "official_catalog_import",
        "ok": output.status != 0 && refused,
        "exit": output.status,
        "stderr": stderr.trim()
    }))
}

fn strip_storage(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, child) in map {
                if key == "stored_physical" || key == "stored_namespace" {
                    continue;
                }
                out.insert(key.clone(), strip_storage(child));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(strip_storage).collect()),
        other => other.clone(),
    }
}

struct BuilderCall<'a> {
    bins: &'a Value,
    catalog: &'a Path,
    target: &'a PgTarget,
    login: &'a str,
    login_password: &'a str,
    origin: &'a str,
    bearer: &'a str,
    price: &'a str,
    deadline_ms: u64,
    reviewer: &'a str,
    reviewed_at: &'a str,
    dataset: &'a str,
    role: &'a str,
    out_dir: &'a Path,
    cache: &'a Path,
}

fn call_builder(call: BuilderCall<'_>) -> Result<Value, String> {
    let BuilderCall {
        bins,
        catalog,
        target,
        login,
        login_password,
        origin,
        bearer,
        price,
        deadline_ms,
        reviewer,
        reviewed_at,
        dataset,
        role,
        out_dir,
        cache,
    } = call;
    fs::create_dir_all(out_dir).map_err(|err| err.to_string())?;
    fs::create_dir_all(cache).map_err(|err| err.to_string())?;
    let price_value: Value = serde_json::from_str(price).map_err(|err| err.to_string())?;
    let config = json!({
        "protocol": "oo.pg-builder.config/v1",
        "adapter": "loopback_http_local_dev",
        "namespace": "pg-builder",
        "purpose": "generate",
        "sample_policy": "none",
        "temperature": 0,
        "endpoint": {"origin": origin, "path": "/v1/chat/completions", "model_id": "grok-4.7"},
        "secret": {"bearer_file": bearer},
        "declared_price": price_value,
        "budget": {
            "deadline_ms": deadline_ms,
            "max_request_bytes": 262144,
            "max_response_bytes": 262144,
            "max_prompt_tokens": 100000,
            "max_completion_tokens": 100000,
            "max_total_tokens": 200000,
            "max_tables": 16,
            "max_in_flight": 1
        }
    });
    let config_path = out_dir.join("builder-config.json");
    write_json(&config_path, &config)?;
    let settings = out_dir.join("settings.json");
    let visibility = out_dir.join("visibility.json");
    write_json(&settings, &settings_json())?;
    write_json(&visibility, &visibility_json(role))?;
    let port = target.port.to_string();
    let mut command = Command::new(text(bins, "builder")?);
    command.args([
        "sealed",
        "--config",
        config_path.to_str().ok_or("path")?,
        "--catalog",
        catalog.to_str().ok_or("path")?,
        "--settings",
        settings.to_str().ok_or("path")?,
        "--visibility",
        visibility.to_str().ok_or("path")?,
        "--cache",
        cache.to_str().ok_or("path")?,
        "--out",
        out_dir.to_str().ok_or("path")?,
        "--reviewer",
        reviewer,
        "--reviewed-at",
        reviewed_at,
        "--dataset",
        dataset,
        "--target-host",
        &target.host,
        "--target-port",
        &port,
        "--target-database",
        &target.database,
        "--target-user",
        login,
        "--login",
        login,
        "--target-ca",
        &target.ca_file,
        "--target-password",
        login_password,
    ]);
    let output = captured(&mut command)?;
    save_step(out_dir, "builder", &output)?;
    let result = read_json(&out_dir.join("result.json")).unwrap_or(Value::Null);
    let outcome = result.get("outcome").and_then(Value::as_str).unwrap_or("");
    let reason = result.get("reason").and_then(Value::as_str).unwrap_or("");
    let http_status = result.get("http_status").and_then(Value::as_u64).unwrap_or(0);
    let expect = if price.contains("unknown") {
        outcome == "cost_unknown"
    } else if deadline_ms == 1 {
        reason == "timeout"
    } else if bearer.contains("bad") || out_dir.ends_with("builder-unauthorized") {
        http_status == 401
    } else if out_dir.ends_with("builder-cache") {
        outcome != "cost_unknown" && result.get("cache").and_then(Value::as_str) == Some("hit")
    } else {
        output.status == 0 && out_dir.join("role-pack").join("pack.json").is_file()
    };
    Ok(json!({
        "name": out_dir.file_name().and_then(|name| name.to_str()).unwrap_or("builder"),
        "ok": expect,
        "exit": output.status,
        "outcome": outcome,
        "reason": reason,
        "http_status": http_status,
        "cache": result.get("cache").cloned().unwrap_or(Value::Null)
    }))
}

fn call_acl(out_dir: &Path, bins: &Value, target: &PgTarget, login: &str) -> Result<Value, String> {
    let effective = read_json(&out_dir.join("builder").join("effective.json"))?;
    let config_digest = read_json(&out_dir.join("sealed-catalog.json"))?
        .get("config_digest")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let datasets = effective.get("datasets").and_then(Value::as_array).ok_or("effective datasets")?;
    let mut policies = Vec::new();
    for dataset in datasets {
        let name = dataset.get("dataset").and_then(Value::as_str).unwrap_or("");
        let Some((namespace, table)) = name.split_once('.') else {
            continue;
        };
        let columns = dataset.get("columns").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut specs = Vec::new();
        for column in columns {
            let semantic = column.get("semantic_type").and_then(Value::as_str).unwrap_or("");
            if semantic.is_empty() {
                return Ok(json!({"name": "install_column_acl", "ok": false, "error": "empty semantic_type"}));
            }
            specs.push(json!({
                "name": column.get("name").cloned().unwrap_or(Value::Null),
                "status": "confirmed",
                "semantic_type": semantic,
                "pii": {"class": "none"}
            }));
        }
        policies.push(json!({
            "project": SOURCE_ID,
            "config_digest": config_digest,
            "generation": 1,
            "role": "w45",
            "login": login,
            "table": {"namespace": namespace, "name": table, "columns": specs}
        }));
    }
    let body = json!({
        "protocol": "oo.gateway.install-column-acl-cli/v1",
        "policies": policies,
        "endpoint": {
            "host": target.host,
            "port": target.port,
            "database": target.database,
            "user": target.user
        },
        "password_file": target.password_file,
        "ca_file": target.ca_file
    });
    let mut command = Command::new(text(bins, "acl")?);
    let output = captured_stdin(&mut command, &serde_json::to_vec(&body).map_err(|err| err.to_string())?)?;
    save_step(out_dir, "install-column-acl", &output)?;
    Ok(json!({"name": "install_column_acl", "ok": output.status == 0 && !policies.is_empty(), "exit": output.status, "tables": policies.len()}))
}

fn call_operator(
    bins: &Value,
    authority: &Path,
    prefix: &[&str],
    query: Option<&Path>,
    out_dir: &Path,
) -> Result<Value, String> {
    fs::create_dir_all(out_dir).map_err(|err| err.to_string())?;
    let mut command = Command::new(text(bins, "operator")?);
    command.arg(prefix[0]).arg(prefix[1]).arg(authority);
    if let Some(path) = query {
        command.arg("--query").arg(path);
    }
    let output = captured(&mut command)?;
    save_step(out_dir, "operator", &output)?;
    Ok(json!({"name": prefix[0], "ok": output.status == 0, "exit": output.status}))
}

fn cancel_query(
    bins: &Value,
    authority: &Path,
    target: &PgTarget,
    mapped: &[catalog::MappedTable],
    query: &Path,
    out_dir: &Path,
) -> Result<Value, String> {
    let physical = mapped
        .iter()
        .find(|table| table.plan.name == "pk_accounts")
        .map(|table| table.physical.clone())
        .ok_or("pk shadow")?;
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|err| err.to_string())?;
    let lock = runtime.block_on(import::lock_table(target, &physical))?;
    let operator = text(bins, "operator")?;
    let child = Command::new(operator)
        .args(["query", "--authority", authority.to_str().ok_or("path")?, "--query", query.to_str().ok_or("path")?])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    let state_dir = out_dir.join("builder").join("state").join("executions");
    let mut execution = None;
    for _ in 0..40 {
        if let Some(id) = running_execution(&state_dir)? {
            execution = Some(id);
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let cancel_out = if let Some(id) = &execution {
        let mut command = Command::new(operator);
        command.args(["cancel", "--authority", authority.to_str().ok_or("path")?, "--execution", id]);
        Some(captured(&mut command)?)
    } else {
        None
    };
    let finished = child.wait_with_output().map_err(|err| err.to_string())?;
    runtime.block_on(import::unlock(lock));
    let cancel_dir = out_dir.join("cancel");
    fs::create_dir_all(&cancel_dir).map_err(|err| err.to_string())?;
    fs::write(cancel_dir.join("query.stdout"), &finished.stdout).map_err(|err| err.to_string())?;
    fs::write(cancel_dir.join("query.stderr"), &finished.stderr).map_err(|err| err.to_string())?;
    let mark = cancel_out
        .as_ref()
        .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
        .unwrap_or_default();
    if let Some(output) = &cancel_out {
        save_step(&cancel_dir, "cancel-cmd", output)?;
    }
    let sent = mark.contains("\"sent\"") || mark.contains("sent");
    Ok(json!({
        "name": "operator_cancel",
        "ok": execution.is_some() && sent,
        "execution": execution,
        "query_exit": finished.status.code(),
        "cancel_stdout": mark.trim()
    }))
}

fn running_execution(dir: &Path) -> Result<Option<String>, String> {
    if !dir.exists() {
        return Ok(None);
    }
    for entry in fs::read_dir(dir).map_err(|err| err.to_string())? {
        let path = entry.map_err(|err| err.to_string())?.path();
        let value = read_json(&path).unwrap_or(Value::Null);
        if value.get("status").and_then(Value::as_str) == Some("running") {
            return Ok(value.get("execution_id").and_then(Value::as_str).map(str::to_string));
        }
    }
    Ok(None)
}

fn pk_balance(decoded: &Path) -> Result<String, String> {
    let value = read_json(&decoded.join("pk_accounts.json"))?;
    let rows = value.get("rows").and_then(Value::as_array).ok_or("rows")?;
    for row in rows {
        if row.get("account_id").and_then(Value::as_i64) == Some(300) {
            return Ok(row.get("balance").and_then(Value::as_str).unwrap_or("").to_string());
        }
    }
    Err("balance".to_string())
}

fn settings_json() -> Value {
    json!({
        "project": SOURCE_ID,
        "ontology_version": "ont-w45",
        "catalog_version": "pending",
        "policy": {"id": "pol-w45", "version": "1"},
        "knowledge": {"id": "know-w45", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": SOURCE_ID,
            "locale": "en-US",
            "include_schemas": [NAMESPACE],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": ["dataset_descriptions"]},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    })
}

fn visibility_json(role: &str) -> Value {
    let datasets = catalog::tables()
        .iter()
        .map(|table| {
            json!({
                "dataset": format!("{NAMESPACE}.{}", table.name),
                "row_security": "none",
                "global_stats": "denied",
                "columns": table.columns.iter().map(|column| column.name).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": SOURCE_ID,
        "principal": "w45-operator",
        "role": role,
        "ontology_version": "ont-w45",
        "catalog_version": "pending",
        "policy": {"id": "pol-w45", "version": "1"},
        "knowledge": {"id": "know-w45", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": datasets,
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    })
}

struct Captured {
    status: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn captured(command: &mut Command) -> Result<Captured, String> {
    let output = command.output().map_err(|err| err.to_string())?;
    Ok(Captured {
        status: output.status.code().unwrap_or(1),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

fn captured_stdin(command: &mut Command, stdin: &[u8]) -> Result<Captured, String> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    child.stdin.take().ok_or("stdin")?.write_all(stdin).map_err(|err| err.to_string())?;
    let output = child.wait_with_output().map_err(|err| err.to_string())?;
    Ok(Captured {
        status: output.status.code().unwrap_or(1),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

fn save_step(dir: &Path, name: &str, output: &Captured) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    fs::write(dir.join(format!("{name}.stdout")), &output.stdout).map_err(|err| err.to_string())?;
    fs::write(dir.join(format!("{name}.stderr")), &output.stderr).map_err(|err| err.to_string())?;
    fs::write(dir.join(format!("{name}.exit")), format!("{}\n", output.status)).map_err(|err| err.to_string())?;
    Ok(())
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|err| err.to_string())
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|err| err.to_string())?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|err| err.to_string())
}

fn object<'a>(value: &'a Value, key: &str) -> Result<&'a Value, String> {
    value.get(key).filter(|item| item.is_object()).ok_or_else(|| key.to_string())
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| key.to_string())
}

fn path_of(value: &Value, key: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(text(value, key)?))
}

fn require(value: &Value, key: &str, expected: &str) -> Result<(), String> {
    if text(value, key)? != expected {
        return Err(key.to_string());
    }
    Ok(())
}
