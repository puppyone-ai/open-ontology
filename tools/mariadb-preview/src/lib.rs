//! MariaDB sealed import and controlled preview. The operator receipt writer is not public.

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

use crate::config::{write_json, Config, Phase};

pub mod capture;
pub mod c5;
pub mod cell;
pub mod config;
pub mod gates;
pub mod import;
pub mod model;
pub mod native;
pub mod preflight;
pub mod witness;

pub async fn builder(config: &Config) -> Phase {
    let catalog_path = config.evidence.join("catalog.json");
    let catalog_text = match std::fs::read_to_string(&catalog_path) {
        Ok(text) => text,
        Err(err) => return finish(config, "builder", false, json!({ "step": "catalog", "error": err.to_string() })),
    };
    let prompt = match gates::column_prompt(&catalog_text) {
        Ok(value) => value,
        Err(err) => return finish(config, "builder", false, json!({ "step": "prompt", "error": err })),
    };
    let schema_digest = prompt.get("schema_digest").and_then(Value::as_str).unwrap_or("").to_string();
    let identity_hash = prompt.get("identity_hash").and_then(Value::as_str).unwrap_or("").to_string();
    let columns = prompt.get("columns").cloned().unwrap_or(Value::Null);
    let decided = model::decide(config, &columns, &schema_digest, &identity_hash).await;
    if !decided.ok {
        return finish(config, "builder", false, json!({ "step": "model", "model": decided.detail }));
    }
    let decisions = match std::fs::read_to_string(config.evidence.join("decisions.json")) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(value) => value,
            Err(err) => return finish(config, "builder", false, json!({ "step": "decisions", "error": err.to_string() })),
        },
        Err(err) => return finish(config, "builder", false, json!({ "step": "decisions", "error": err.to_string() })),
    };
    let reviewed = match gates::reviewed(&catalog_text, &decisions) {
        Ok(value) => value,
        Err(err) => return finish(config, "builder", false, json!({ "step": "reviewed", "error": err })),
    };
    if let Err(err) = write_json(&config.evidence.join("reviewed.json"), &reviewed) {
        return finish(config, "builder", false, json!({ "step": "reviewed-file", "error": err }));
    }
    let catalog: Value = serde_json::from_str(&catalog_text).unwrap_or(Value::Null);
    let import_hash = catalog.get("input_digest").and_then(Value::as_str).unwrap_or("").to_string();
    let rows = pack_rows(&catalog);
    let packed = gates::role_and_pack(config, &reviewed, &import_hash, &rows);
    finish(
        config,
        "builder",
        packed.ok,
        json!({
            "schema_digest": schema_digest,
            "identity_hash": identity_hash,
            "import_hash": import_hash,
            "role_pack": packed.detail,
            "declared_cost": { "status": "unknown" },
        }),
    )
}

pub async fn gates_run(config: &Config) -> Phase {
    let admission = gates::admission(config);
    let negative = model::negative(config).await;
    let operator = gates::operator_record();
    finish(
        config,
        "gates",
        admission.ok && negative.ok,
        json!({
            "admission": admission.ok,
            "model_negative": negative.ok,
            "operator": operator,
            "declared_cost": { "status": "unknown" },
        }),
    )
}

pub async fn audit(config: &Config) -> Phase {
    if let Err(err) = config::write_bytes(&config.audit_request, b"export\n") {
        return finish(config, "audit", false, json!({ "error": err }));
    }
    let started = std::time::Instant::now();
    let mut text = String::new();
    while started.elapsed() < Duration::from_secs(120) {
        if let Ok(bytes) = std::fs::read(&config.audit_tsv)
            && !bytes.is_empty()
        {
            text = String::from_utf8_lossy(&bytes).into_owned();
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    if text.is_empty() {
        return finish(config, "audit", false, json!({ "error": "audit tsv missing" }));
    }
    match mariadb_probe::audit::audit_log(&text) {
        Ok(counts) => finish(config, "audit", true, counts_json(&counts)),
        Err(err) => finish(
            config,
            "audit",
            false,
            json!({ "code": err.code, "fact": config::redact(&err.fact, &[]) }),
        ),
    }
}

pub async fn all(config: &Config) -> Vec<Phase> {
    let mut phases = Vec::new();
    phases.push(preflight::run(config).await);
    phases.push(import::run(config).await);
    phases.push(builder(config).await);
    phases.push(gates_run(config).await);
    phases.push(audit(config).await);
    phases.push(preflight::broken(config).await);
    let _ = write_json(&config.evidence.join("phases").join("all.json"), &phases);
    phases
}

pub fn executable_ok(phases: &[Phase]) -> bool {
    phases.iter().all(|phase| phase.ok)
}

fn pack_rows(catalog: &Value) -> Value {
    let rows = catalog
        .get("tables")
        .and_then(Value::as_array)
        .map(|tables| {
            tables
                .iter()
                .map(|table| {
                    json!({
                        "schema": table.get("logical_schema").and_then(Value::as_str).unwrap_or(""),
                        "name": table.get("logical_table").and_then(Value::as_str).unwrap_or(""),
                        "rows": table.get("accepted_rows").and_then(Value::as_u64).unwrap_or(0),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Value::Array(rows)
}

fn counts_json(counts: &mariadb_probe::audit::AuditCounts) -> Value {
    json!({
        "rows": counts.rows,
        "connect": counts.connect,
        "read": counts.read,
        "session": counts.session,
        "cancel": counts.cancel,
        "execute": counts.execute,
        "write": counts.write,
        "empty": counts.empty,
        "unexpected": counts.unexpected,
    })
}

fn finish(config: &Config, name: &str, ok: bool, detail: Value) -> Phase {
    let phase = Phase::new(name, ok, detail);
    let _ = phase.save(&config.evidence);
    phase
}

pub fn load_config(path: &Path) -> Result<Config, String> {
    Config::load(path)
}
