//! Admission, role pack, and the operator receipt gap. The pack writer is not public.

use oo_builder::{
    REVIEWED_SEALED_IMPORT_COMMAND_PROTOCOL, SealedImportDraft, compile_reviewed_sealed_import,
    compile_role_pack,
};
use serde_json::{Value, json};
use source_admission::{
    Budget, CONFIG_PROTOCOL, CaptureMethod, CostModel, DebeziumOptions, DeclaredCapability, Driver,
    Permission, REPORT_PROTOCOL, SourceConfig, TargetReport, WriteOptions, admit, decision_json,
};

use crate::config::{self, Config, Phase};

pub const OPERATOR_GAP: &str = "tools/minimal-product-loop/src/pack.rs write_sealed_pack is pub inside a private module and is not re-exported from lib.rs (comment: \"lib.rs does not re-export write_sealed_pack. 411 owns that file\"). The only caller is run::write_standard_pack, reached through public run(), which requires doctor, ready_bins, and a PostgreSQL local-pg-mirror sidecar (mirror-invoke.json). Without that sidecar, run() uses count_source and write_draft and never calls write_sealed_pack. Operator bind_preview verifies a receipt that only write_sealed_pack produces. Operator tests do not call the writer; they consume an env-provided receipt. Editing minimal-product-loop, faking the PG mirror, or hand-authoring the receipt is forbidden. Record operator describe/query/cancel as not executed.";

const EFFECTIVE_PROTOCOL: &str = "oo.builder.effective-oos/v1";
const VISIBILITY_PROTOCOL: &str = "oo.builder.role-visibility/v1";
const ROLE_PACK_COMPILER: &str = "oo-builder/role-pack/v1";

pub fn admission(config: &Config) -> Phase {
    let report = base_report(&config.image_digest);
    let source = base_config();
    let positive = admit(&report, &source);
    let mut unknown_unit_report = report.clone();
    let mut unknown_unit_config = source.clone();
    unknown_unit_report.cost_model.unit = "Widgets".to_string();
    unknown_unit_config.budget.unit = "Widgets".to_string();
    let unknown_unit = admit(&unknown_unit_report, &unknown_unit_config);
    let mut unknown_compat_report = report.clone();
    let mut unknown_compat_config = source.clone();
    unknown_compat_report.compat_mode = "mariadb-fork".to_string();
    unknown_compat_config.compat_mode = "mariadb-fork".to_string();
    let unknown_compat = admit(&unknown_compat_report, &unknown_compat_config);
    let mut zero_report = report.clone();
    let mut zero_config = source.clone();
    zero_report.cost_model.confidence = "unknown".to_string();
    zero_config.budget.confidence = "unknown".to_string();
    zero_config.budget.hard_limit = 0;
    let unknown_zero = admit(&zero_report, &zero_config);
    let mut nonzero_report = report.clone();
    let mut nonzero_config = source;
    nonzero_report.cost_model.confidence = "unknown".to_string();
    nonzero_config.budget.confidence = "unknown".to_string();
    nonzero_config.budget.hard_limit = 1;
    let unknown_nonzero = admit(&nonzero_report, &nonzero_config);
    let ok = positive.admitted
        && !unknown_unit.admitted
        && !unknown_compat.admitted
        && !unknown_zero.admitted
        && unknown_nonzero.admitted;
    let detail = json!({
        "positive": summary(&positive),
        "unknown_unit": summary(&unknown_unit),
        "unknown_compat": summary(&unknown_compat),
        "unknown_confidence_zero": summary(&unknown_zero),
        "unknown_confidence_nonzero": summary(&unknown_nonzero),
        "declared_cost": { "status": "unknown" },
    });
    let _ = config::write_json(&config.evidence.join("admission-decision.json"), &detail);
    if let Ok(bytes) = decision_json(&positive) {
        let _ = config::write_bytes(&config.evidence.join("admission-positive.json"), &bytes);
    }
    let phase = Phase::new("admission", ok, detail);
    let _ = phase.save(&config.evidence);
    phase
}

fn summary(decision: &source_admission::Decision) -> Value {
    json!({
        "admitted": decision.admitted,
        "blocks": decision.blocks.iter().map(|block| json!({"code": block.code, "fact": block.fact})).collect::<Vec<_>>(),
        "zero_write_proven": decision.zero_write.proven,
        "zero_write_basis": decision.zero_write.basis,
    })
}

fn base_report(image_digest: &str) -> TargetReport {
    TargetReport {
        protocol: REPORT_PROTOCOL.to_string(),
        target_id: "w39_target".to_string(),
        source_id: config::SOURCE_ID.to_string(),
        secret_ref: "secret://w39/reader".to_string(),
        driver: Driver {
            kind: "native".to_string(),
            name: "mariadb-preview".to_string(),
            version: "0.1.0".to_string(),
            digest: image_digest.to_string(),
        },
        compat_mode: "mariadb-native".to_string(),
        license_flags: Vec::new(),
        permissions: vec![Permission {
            name: "SELECT".to_string(),
            status: "known".to_string(),
            granted: true,
            scope: None,
        }],
        native: None,
        capabilities: vec![DeclaredCapability {
            name: "Catalog".to_string(),
            cursor: None,
            feed: None,
            cdc: None,
            token_order: None,
            budget_unit: None,
            level: "verified".to_string(),
            write_class: "R0".to_string(),
            consistency: "S1".to_string(),
        }],
        capture_methods: vec![CaptureMethod {
            method: "C1".to_string(),
            write_class: "R0".to_string(),
            consistency: "S1".to_string(),
            token_order: "None".to_string(),
            retention_window: None,
            cursor: None,
            feed: None,
            cdc: None,
            delete_detection: None,
        }],
        cost_model: CostModel {
            unit: "Rows".to_string(),
            confidence: "unknown".to_string(),
            estimate: "0".to_string(),
        },
        r1w: None,
        capture_witness: None,
    }
}

fn base_config() -> SourceConfig {
    SourceConfig {
        protocol: CONFIG_PROTOCOL.to_string(),
        target_id: "w39_target".to_string(),
        source_id: config::SOURCE_ID.to_string(),
        secret_ref: "secret://w39/reader".to_string(),
        compat_mode: "mariadb-native".to_string(),
        permissions: vec![Permission {
            name: "SELECT".to_string(),
            status: "known".to_string(),
            granted: true,
            scope: None,
        }],
        budget: Budget {
            unit: "Rows".to_string(),
            hard_limit: 1,
            confidence: "unknown".to_string(),
        },
        write_options: WriteOptions {
            signal_table: false,
            heartbeat_write: false,
            create_stream: false,
            create_trigger: false,
        },
        debezium: DebeziumOptions {
            signal_data_collection: None,
            heartbeat_action_query: None,
            snapshot_mode: "never".to_string(),
            create_stream: false,
            create_trigger: false,
        },
        requested_write_class: "R0".to_string(),
        r1w: None,
        native: None,
    }
}

pub fn reviewed(catalog_text: &str, decisions: &Value) -> Result<Value, String> {
    let catalog: Value = serde_json::from_str(catalog_text).map_err(|err| err.to_string())?;
    let draft =
        oo_builder::compile_sealed_import_catalog(catalog_text).map_err(|err| err.to_string())?;
    let decisions = align_decisions(&draft, decisions);
    let settings = json!({
        "project": "mariadb_w39",
        "ontology_version": "ont-w39",
        "catalog_version": draft.identity_hash,
        "policy": {"id": "pol_w39", "version": "1"},
        "knowledge": {"id": "know_w39", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "mariadb_w39",
            "locale": "en-US",
            "include_schemas": catalog_schemas(&catalog),
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    });
    let command = json!({
        "protocol": REVIEWED_SEALED_IMPORT_COMMAND_PROTOCOL,
        "catalog": catalog,
        "settings": settings,
        "decisions": decisions,
    });
    let text = serde_json::to_string(&command).map_err(|err| err.to_string())?;
    let compiled = compile_reviewed_sealed_import(&text).map_err(|err| err.to_string())?;
    serde_json::from_str(&compiled).map_err(|err| err.to_string())
}

/// Restore dataset and column from a unique draft fingerprint.
///
/// The model may shorten `oo_synth.k_unsigned` to `oo_synth`. That repeats
/// `(dataset, column)` and the compiler rejects the command. A missing or
/// ambiguous fingerprint is left unchanged so the compiler still refuses it.
fn align_decisions(draft: &SealedImportDraft, decisions: &Value) -> Value {
    let Some(rows) = decisions.as_array() else {
        return decisions.clone();
    };
    let mut aligned = Vec::with_capacity(rows.len());
    for row in rows {
        let mut copy = row.clone();
        if let Some(fingerprint) = row.get("fact_fingerprint").and_then(Value::as_str) {
            let hits: Vec<usize> = draft
                .columns
                .iter()
                .enumerate()
                .filter(|(_, column)| column.fact_fingerprint == fingerprint)
                .map(|(index, _)| index)
                .take(2)
                .collect();
            if let [index] = hits.as_slice() {
                let column = &draft.columns[*index];
                if let Some(object) = copy.as_object_mut() {
                    object.insert("dataset".to_string(), Value::String(column.dataset.clone()));
                    object.insert("column".to_string(), Value::String(column.name.clone()));
                }
            }
        }
        aligned.push(copy);
    }
    Value::Array(aligned)
}

pub fn column_prompt(catalog_text: &str) -> Result<Value, String> {
    let draft =
        oo_builder::compile_sealed_import_catalog(catalog_text).map_err(|err| err.to_string())?;
    let columns = draft
        .columns
        .iter()
        .map(|column| {
            json!({
                "dataset": column.dataset,
                "column": column.name,
                "fact_fingerprint": column.fact_fingerprint,
                "data_type": column.target_type,
                "nullable": true,
                "key": column.key,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "identity_hash": draft.identity_hash,
        "schema_digest": draft.schema_digest,
        "columns": columns,
    }))
}

pub fn role_and_pack(config: &Config, reviewed: &Value, import_hash: &str, rows: &Value) -> Phase {
    let Some(effective) = reviewed.get("effective_oos") else {
        return Phase::new(
            "role-pack",
            false,
            json!({ "error": "effective document missing" }),
        );
    };
    if effective.get("protocol").and_then(Value::as_str) != Some(EFFECTIVE_PROTOCOL) {
        return Phase::new("role-pack", false, json!({ "error": "effective protocol" }));
    }
    let effective_text = serde_json::to_string(effective).unwrap_or_default();
    let hidden = reviewed.get("hidden").and_then(Value::as_array);
    let visibility = visibility_for(effective, hidden);
    let visibility_text = serde_json::to_string(&visibility).unwrap_or_default();
    let packed = compile_role_pack(&effective_text, &visibility_text);
    let mut negative_visibility = visibility;
    negative_visibility["role"] = json!("not_a_role");
    negative_visibility["protocol"] = json!(VISIBILITY_PROTOCOL);
    let negative_text = serde_json::to_string(&negative_visibility).unwrap_or_default();
    let negative = compile_role_pack(&effective_text, &negative_text);
    let negative_ok = match &negative {
        Err(err) => err.to_string().contains("role"),
        Ok(_) => false,
    };
    let mut pack_ok = false;
    let mut pack_error = None;
    if let Ok(pack) = &packed {
        let dir = config.evidence.join("role-pack");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("w39.json"), &pack.artifact);
        match sealed_pack(reviewed, import_hash, rows) {
            Ok(input) => {
                pack_ok = true;
                let _ = config::write_json(&config.evidence.join("pack-input.json"), &input);
            }
            Err(err) => pack_error = Some(err),
        }
    }
    let ok = packed.is_ok() && negative_ok && pack_ok;
    let phase = Phase::new(
        "role-pack",
        ok,
        json!({
            "packed": packed.is_ok(),
            "pack_error": packed.as_ref().err().map(|err| err.to_string()),
            "negative_role_ok": negative_ok,
            "negative_role": negative.as_ref().err().map(|err| err.to_string()),
            "pack_input": pack_ok,
            "pack_input_error": pack_error,
            "operator_describe": "not_executed",
            "operator_query": "not_executed",
            "operator_cancel": "not_executed",
            "findings": OPERATOR_GAP,
        }),
    );
    let _ = phase.save(&config.evidence);
    phase
}

fn visibility_for(effective: &Value, hidden: Option<&Vec<Value>>) -> Value {
    let hidden_key = |dataset: &str, column: &str| {
        hidden.is_some_and(|rows| {
            rows.iter().any(|row| {
                row.get("dataset").and_then(Value::as_str) == Some(dataset)
                    && row.get("column").and_then(Value::as_str) == Some(column)
            })
        })
    };
    let mut datasets = Vec::new();
    if let Some(rows) = effective.get("datasets").and_then(Value::as_array) {
        for dataset in rows {
            let name = dataset.get("dataset").and_then(Value::as_str).unwrap_or("");
            let mut columns = Vec::new();
            if let Some(cols) = dataset.get("columns").and_then(Value::as_array) {
                for column in cols {
                    let column_name = column.get("name").and_then(Value::as_str).unwrap_or("");
                    let classified = column.get("pii").is_some()
                        && !column.get("pii").is_some_and(Value::is_null);
                    if classified && !hidden_key(name, column_name) && !column_name.is_empty() {
                        columns.push(Value::String(column_name.to_string()));
                    }
                }
            }
            if !columns.is_empty() {
                datasets.push(json!({
                    "dataset": name,
                    "row_security": "none",
                    "global_stats": "denied",
                    "columns": columns,
                }));
            }
        }
    }
    json!({
        "protocol": VISIBILITY_PROTOCOL,
        "project": effective.get("project").cloned().unwrap_or(json!("mariadb_w39")),
        "principal": "w39_reader",
        "role": "oo_r_w39_reader",
        "ontology_version": effective.get("ontology_version").cloned().unwrap_or(json!("ont-w39")),
        "catalog_version": effective.get("catalog_version").cloned().unwrap_or(json!("")),
        "policy": effective.get("policy").cloned().unwrap_or(json!({"id": "pol_w39", "version": "1"})),
        "knowledge": effective.get("knowledge").cloned().unwrap_or(json!({"id": "know_w39", "version": "1"})),
        "issued_for_compiler": ROLE_PACK_COMPILER,
        "datasets": datasets,
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 64},
    })
}

fn catalog_schemas(catalog: &Value) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(tables) = catalog.get("tables").and_then(Value::as_array) {
        for table in tables {
            if let Some(name) = table.get("logical_schema").and_then(Value::as_str)
                && !name.is_empty()
                && !names.iter().any(|item| item == name)
            {
                names.push(name.to_string());
            }
        }
    }
    names
}

fn sealed_pack(reviewed: &Value, import_hash: &str, tables: &Value) -> Result<Value, String> {
    let schema = tables
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("schema"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let catalog = json!({
        "protocol": "oo.source.pgsql/v1",
        "schema": schema,
        "foreign_keys": [],
        "note": "local stored shape after sealed import",
    });
    let profile_tables = tables.as_array().map(|rows| {
        rows.iter()
            .map(|row| {
                json!({
                    "schema": row.get("schema").and_then(Value::as_str).unwrap_or(""),
                    "name": row.get("name").and_then(Value::as_str).unwrap_or(""),
                    "reltuples": row.get("rows").map(|value| match value {
                        Value::Number(number) => number.to_string(),
                        other => other.to_string(),
                    }).unwrap_or_else(|| "0".to_string()),
                })
            })
            .collect::<Vec<_>>()
    });
    let profile = json!({
        "protocol": "oo.pg-builder.profile/v1",
        "sample_policy": "none",
        "tables": profile_tables,
    });
    let generation = json!({
        "protocol": "oo.pg-builder.generation/v1",
        "outcome": "validated",
        "transport": {"model_id": "grok-4.7", "declared_cost": {"status": "unknown"}},
    });
    let identity = reviewed
        .pointer("/provenance/identity_hash")
        .and_then(Value::as_str)
        .unwrap_or(import_hash);
    let binding = json!({
        "import_generation": 1,
        "import_hash": import_hash,
        "manifest_sha256": identity,
        "snapshot_id": "mariadb-w39",
        "source_id": config::SOURCE_ID,
        "target_config_sha256": reviewed.pointer("/provenance/config_digest").and_then(Value::as_str).unwrap_or(import_hash),
    });
    if reviewed
        .pointer("/provenance/source_engine")
        .and_then(Value::as_str)
        != Some("mariadb")
    {
        return Err("reviewed source engine".to_string());
    }
    minimal_product_loop::sealed_pack_input(&catalog, &profile, &generation, &binding, tables)
        .map_err(|err| err.to_string())
}

pub fn operator_record() -> Value {
    json!({
        "describe": "not_executed",
        "query": "not_executed",
        "cancel": "not_executed",
        "findings": OPERATOR_GAP,
    })
}
