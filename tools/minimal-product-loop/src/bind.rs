//! Sealed-manifest facts and the development query-pg document.
//!
//! The document is the real `oo.gateway.query-pg/v1` shape. It is not a reviewed
//! policy and it is not a publication. Column class `auto_accepted` is the
//! gateway's development admission for this public synthetic fixture. The fact
//! file says that plainly.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use crate::config::{Config, ConfigError};
use crate::source::MappingRead;

pub const DEV_ROLE_SLUG: &str = "devread";
pub const BULK_IMPORT_HASH: &str = "49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73";

pub fn dev_role_name() -> String {
    format!("oo_r_{DEV_ROLE_SLUG}")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedTable {
    pub schema: String,
    pub name: String,
    pub rows: i64,
    pub physical_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedCopy {
    pub import_hash: String,
    pub snapshot_id: String,
    pub source_id: String,
    pub generation: i64,
    pub target_adopt_generation: i64,
    pub plan_sha256: String,
    pub rows: i64,
    pub tables: Vec<SealedTable>,
}

pub fn load_sealed(config_path: &Path) -> Result<SealedCopy, ConfigError> {
    let spec = crate::copy_invoke::open_sidecar(config_path)?;
    let plan = read_json(&spec.plan, 1_048_576, "the sealed plan")?;
    let manifest = read_json(&spec.manifest, 1_048_576, "the sealed manifest")?;
    let source_id = text(&plan, "source_id", "the sealed plan")?;
    let generation = plan.get("generation").and_then(Value::as_i64).filter(|value| *value >= 1).ok_or_else(|| ConfigError::new("copy.producer", "the sealed plan generation is not allowed"))?;
    let plan_sha256 = hex_text(&plan, "plan_sha256", "the sealed plan")?;
    if manifest.get("import_hash").and_then(Value::as_str) != Some(spec.import_hash.as_str()) {
        return Err(ConfigError::new("copy.producer", "the sealed manifest is a different import"));
    }
    if manifest.get("mirror_state").and_then(Value::as_str) != Some("complete") || manifest.get("customer_complete").and_then(Value::as_bool) != Some(false) {
        return Err(ConfigError::new("copy.producer", "the sealed manifest is not a complete unreleased copy"));
    }
    if manifest.get("plan_sha256").and_then(Value::as_str) != Some(plan_sha256) {
        return Err(ConfigError::new("copy.producer", "the sealed manifest does not match the plan"));
    }
    // Target adopt generation and the source plan generation are different counters.
    // Mappings are stored under the plan generation.
    if manifest.get("import_generation").and_then(Value::as_i64).filter(|value| *value >= 1).is_none() {
        return Err(ConfigError::new("copy.producer", "the sealed target generation is not allowed"));
    }
    let snapshot_id = text(&manifest, "snapshot_id", "the sealed manifest")?;
    if snapshot_id.len() > 64 || snapshot_id.chars().any(|ch| ch.is_whitespace()) {
        return Err(ConfigError::new("copy.producer", "the snapshot id is not allowed"));
    }
    let rows = manifest.get("rows").and_then(Value::as_i64).filter(|value| *value >= 0).ok_or_else(|| ConfigError::new("copy.producer", "the sealed row count is not allowed"))?;
    let listed = manifest.get("tables_completed").and_then(Value::as_array).ok_or_else(|| ConfigError::new("copy.producer", "the sealed manifest has no completed tables"))?;
    let mut tables = Vec::new();
    for item in listed {
        let row = item.as_object().ok_or_else(|| ConfigError::new("copy.producer", "a sealed table is not an object"))?;
        let table_rows = row.get("rows").and_then(Value::as_i64).filter(|value| *value >= 0).ok_or_else(|| ConfigError::new("copy.producer", "a sealed table row count is not allowed"))?;
        tables.push(SealedTable {
            schema: text(item, "schema", "the sealed manifest")?.to_owned(),
            name: text(item, "name", "the sealed manifest")?.to_owned(),
            rows: table_rows,
            physical_name: row.get("physical_name").and_then(Value::as_str).unwrap_or("").to_owned(),
        });
    }
    if tables.is_empty() {
        return Err(ConfigError::new("copy.producer", "the sealed manifest has no completed tables"));
    }
    Ok(SealedCopy {
        import_hash: spec.import_hash,
        snapshot_id: snapshot_id.to_owned(),
        source_id: source_id.to_owned(),
        generation,
        target_adopt_generation: manifest["import_generation"].as_i64().ok_or_else(|| ConfigError::new("copy.producer", "the target generation is absent"))?,
        plan_sha256: plan_sha256.to_owned(),
        rows,
        tables,
    })
}

/// Digests the pack binder requires. `load_sealed` stays valid when a fixture omits them.
pub fn sealed_binding_hashes(config_path: &Path) -> Result<(String, String), ConfigError> {
    let spec = crate::copy_invoke::open_sidecar(config_path)?;
    let manifest = read_json(&spec.manifest, 1_048_576, "the sealed manifest")?;
    let manifest_sha256 = hex_text(&manifest, "manifest_sha256", "the sealed manifest")?.to_owned();
    let target_config_sha256 = hex_text(&manifest, "target_config_sha256", "the sealed manifest")?.to_owned();
    Ok((manifest_sha256, target_config_sha256))
}

pub fn grantable(type_name: &str, typmod: i32) -> bool {
    match type_name {
        "bool" | "int2" | "int4" | "int8" | "text" => true,
        "numeric" => numeric_precision(typmod).is_some_and(|precision| (1..=38).contains(&precision)),
        _ => false,
    }
}

fn numeric_precision(typmod: i32) -> Option<i32> {
    if typmod < 0 {
        return None;
    }
    Some((typmod.wrapping_sub(4) >> 16) & 0xFFFF)
}

pub fn write_dev_policy(config: &Config, sealed: &SealedCopy, mapping: &MappingRead) -> Result<(), ConfigError> {
    let mut policies = Vec::new();
    let mut omitted = Vec::new();
    for table in &mapping.tables {
        let mut columns = Vec::new();
        for column in &table.columns {
            if column.name == "_oo_pos" {
                return Err(ConfigError::new("policy", "a metadata column was selected"));
            }
            if grantable(&column.type_name, column.typmod) {
                columns.push(json!({
                    "name": column.name,
                    "status": "auto_accepted",
                    "semantic_type": column.type_name,
                    "pii": {"class": "none"}
                }));
            } else {
                omitted.push(json!({
                    "table": format!("{}.{}", table.logical_schema, table.logical_table),
                    "column": column.name,
                    "type_name": column.type_name,
                    "reason": "unsupported_type"
                }));
            }
        }
        if columns.is_empty() {
            return Err(ConfigError::new("policy", "a sealed table has no admitted column"));
        }
        policies.push(json!({
            "project": sealed.source_id,
            "config_digest": mapping.config_digest,
            "generation": sealed.generation,
            "role": DEV_ROLE_SLUG,
            "login": config.username,
            "table": {
                "namespace": table.logical_schema,
                "name": table.logical_table,
                "columns": columns
            }
        }));
    }
    if policies.is_empty() {
        return Err(ConfigError::new("policy", "the development policy has no table"));
    }
    let document = json!({
        "endpoint": {
            "host": config.host,
            "port": config.port,
            "database": config.database,
            "user": config.username
        },
        "password_file": config.password_file.display().to_string(),
        "ca_file": config.ca_file.display().to_string(),
        "identity": {
            "project": sealed.source_id,
            "role": dev_role_name(),
            "policy_id": mapping.config_digest
        },
        "policies": policies,
        "catalog": {
            "max_tables": 8,
            "max_columns": 64,
            "max_functions": 8,
            "max_operators": 64,
            "large_table_row_threshold": 1_000_000,
            "unknown_rows": "refuse"
        }
    });
    let fact = json!({
        "protocol": "oo.minimal-product-loop.dev-policy-fact/v1",
        "publication": "not_published",
        "llm_review": "unreviewed",
        "human_review": false,
        "classification": "development_auto_accepted",
        "operator_label": config.operator_label,
        "operator_at": config.operator_at,
        "import_hash": sealed.import_hash,
        "snapshot_id": sealed.snapshot_id,
        "source_id": sealed.source_id,
        "generation": sealed.generation,
        "mapping_generation": sealed.generation,
        "target_adopt_generation": sealed.target_adopt_generation,
        "config_digest": mapping.config_digest,
        "declared_foreign_key": "not_declared",
        "role": dev_role_name(),
        "role_state": mapping.role_state,
        "login": config.username,
        "omitted": omitted,
        "note": "auto_accepted is not a human review and not a publication"
    });
    fs::create_dir_all(&config.work).map_err(|_| ConfigError::new("policy", "the work directory could not be created"))?;
    fs::write(config.work.join("dev-policy.json"), serde_json::to_vec_pretty(&document).map_err(|_| ConfigError::new("policy", "the development policy could not be encoded"))?)
        .map_err(|_| ConfigError::new("policy", "the development policy could not be written"))?;
    fs::write(config.work.join("dev-policy-fact.json"), serde_json::to_vec_pretty(&fact).map_err(|_| ConfigError::new("policy", "the development policy fact could not be encoded"))?)
        .map_err(|_| ConfigError::new("policy", "the development policy fact could not be written"))?;
    Ok(())
}

fn read_json(path: &Path, max: u64, _label: &str) -> Result<Value, ConfigError> {
    let meta = fs::symlink_metadata(path).map_err(|_| ConfigError::new("copy.producer", "a sealed file is absent"))?;
    if !meta.file_type().is_file() || meta.len() == 0 || meta.len() > max {
        return Err(ConfigError::new("copy.producer", "a sealed file is not allowed"));
    }
    let bytes = fs::read(path).map_err(|_| ConfigError::new("copy.producer", "a sealed file could not be read"))?;
    serde_json::from_slice(&bytes).map_err(|_| ConfigError::new("copy.producer", "a sealed file is not JSON")).and_then(|value: Value| {
        if value.is_object() {
            Ok(value)
        } else {
            Err(ConfigError::new("copy.producer", "a sealed file is not one object"))
        }
    })
}

fn text<'a>(value: &'a Value, key: &str, _label: &'static str) -> Result<&'a str, ConfigError> {
    value.get(key).and_then(Value::as_str).filter(|text| !text.is_empty() && !text.chars().any(|ch| ch.is_whitespace())).ok_or_else(|| ConfigError::new("copy.producer", "a sealed field is not allowed"))
}

fn hex_text<'a>(value: &'a Value, key: &str, label: &'static str) -> Result<&'a str, ConfigError> {
    let text = text(value, key, label)?;
    if text.len() == 64 && text.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()) {
        Ok(text)
    } else {
        Err(ConfigError::new("copy.producer", "a sealed digest is not allowed"))
    }
}

#[cfg(test)]
mod tests {
    use super::{grantable, load_sealed, write_dev_policy, BULK_IMPORT_HASH};
    use crate::config::load_config;
    use crate::source::{LiveColumn, LiveTable, MappingRead};
    use serde_json::{Map, Value};
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    #[test]
    fn numeric_38_is_admitted_and_time_and_bytea_are_not() {
        assert!(grantable("int8", -1));
        assert!(grantable("numeric", 2_490_376));
        assert!(grantable("text", -1));
        assert!(!grantable("time", -1));
        assert!(!grantable("bytea", -1));
        assert!(!grantable("numeric", -1));
    }

    #[test]
    fn sealed_plan_fields_do_not_keep_the_secret_ref() {
        let dir = std::env::temp_dir().join(format!("oo-a411-sealed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let plan = dir.join("plan.json");
        let manifest = dir.join("manifest.json");
        let target = dir.join("target.json");
        let binary = dir.join("local-pg-mirror");
        fs::write(
            &plan,
            br#"{"source_id":"fixk","generation":1,"plan_sha256":"cf9845c5c257a728cd95e3125b5b2500c0724980babd25df0ad32e5e046773ea","secret_ref":"secret://not-copied","owner":"hidden"}"#,
        )
        .expect("plan");
        fs::write(
            &manifest,
            format!(
                "{{\"import_hash\":\"{BULK_IMPORT_HASH}\",\"mirror_state\":\"complete\",\"customer_complete\":false,\"plan_sha256\":\"cf9845c5c257a728cd95e3125b5b2500c0724980babd25df0ad32e5e046773ea\",\"import_generation\":1,\"snapshot_id\":\"00000004-00000013-1\",\"rows\":100000,\"tables_completed\":[{{\"schema\":\"public\",\"name\":\"batch_a\",\"rows\":2000,\"physical_name\":\"pt_a\"}}]}}"
            ),
        )
        .expect("manifest");
        fs::write(&target, b"{}\n").expect("target");
        let mut binary_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o755).open(&binary).expect("bin");
        binary_file.write_all(b"#!/bin/sh\n").expect("script");
        let sidecar = format!(
            "{{\"protocol\":\"oo.minimal-product-loop.copy-invoke/v1\",\"binary\":\"{}\",\"plan\":\"{}\",\"manifest\":\"{}\",\"target_config\":\"{}\",\"import_hash\":\"{BULK_IMPORT_HASH}\"}}\n",
            binary.display(),
            plan.display(),
            manifest.display(),
            target.display()
        );
        fs::write(dir.join("mirror-invoke.json"), sidecar).expect("sidecar");
        fs::write(dir.join("config.json"), b"{}\n").expect("config");
        let sealed = load_sealed(&dir.join("config.json")).expect("sealed");
        assert_eq!(sealed.source_id, "fixk");
        assert_eq!(sealed.generation, 1);
        assert_eq!(sealed.rows, 100000);
        fs::write(
            &plan,
            br#"{"source_id":"fixk","generation":2026100901,"plan_sha256":"cf9845c5c257a728cd95e3125b5b2500c0724980babd25df0ad32e5e046773ea","secret_ref":"secret://not-copied","owner":"hidden"}"#,
        )
        .expect("plan generation");
        let sealed = load_sealed(&dir.join("config.json")).expect("target generation may differ");
        assert_eq!(sealed.generation, 2026100901);
        assert_eq!(sealed.tables[0].name, "batch_a");
        assert!(super::sealed_binding_hashes(&dir.join("config.json")).is_err());
        let rendered = format!("{sealed:?}");
        assert!(!rendered.contains("secret://not-copied"));
        assert!(!rendered.contains("hidden"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn development_policy_names_the_gateway_role_and_omits_bytea() {
        let dir = std::env::temp_dir().join(format!("oo-a411-policy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("work")).expect("work");
        fs::write(dir.join("password"), b"pppppppp").expect("password");
        fs::write(dir.join("ca.pem"), b"ca-bytes\n").expect("ca");
        fs::write(dir.join("bearer"), b"bbbbbbbb").expect("bearer");
        let config_path = dir.join("loop.json");
        let body = format!(
            "{{\n\"protocol\":\"oo.minimal-product-loop.config/v1\",\"mode\":\"operator-development\",\"development_preview\":true,\"publication\":\"not_published\",\"llm_review\":\"unreviewed\",\"runtime_acknowledgement\":\"local-process-not-a-sandbox-certificate\",\n\"database\":{{\"type\":\"postgresql\",\"host\":\"oo-a410-tgt\",\"port\":5432,\"name\":\"mirrordb\",\"username\":\"oo_mirror_read\",\"tls\":\"require\",\"secret_ref\":\"secret://pg/a410/reader\",\"password_file\":\"{}\",\"ca_file\":\"{}\"}},\n\"tables\":[{{\"schema\":\"public\",\"table\":\"batch_b\"}}],\n\"llm\":{{\"model_id\":\"grok-4.7\",\"origin\":\"http://127.0.0.1:8317\",\"path\":\"/v1/chat/completions\",\"bearer_file\":\"{}\",\"deadline_ms\":90000}},\n\"gateway\":{{\"oo\":\"/oo\",\"query_pg\":\"/query-pg\",\"policy_file\":\"/policy.json\",\"token_file\":\"/token\"}},\n\"pi\":{{\"executable\":\"/pi\",\"executable_sha256\":\"{}\",\"runtime\":\"/node\",\"runtime_sha256\":\"{}\",\"provider\":\"grok\",\"model\":\"grok-4.7\"}},\n\"paths\":{{\"catalog_bin\":\"/catalog\",\"knowledge\":\"{}\",\"work\":\"{}\"}},\n\"operator\":{{\"label\":\"a411\",\"at\":\"2026-10-09T00:00:00Z\"}}\n}}\n",
            dir.join("password").display(),
            dir.join("ca.pem").display(),
            dir.join("bearer").display(),
            "0".repeat(64),
            "0".repeat(64),
            dir.join("knowledge").display(),
            dir.join("work").display()
        );
        fs::write(&config_path, body).expect("config");
        let config = load_config(&config_path).expect("load");
        let sealed = super::SealedCopy {
            import_hash: BULK_IMPORT_HASH.to_owned(),
            snapshot_id: "00000004-00000013-1".to_owned(),
            source_id: "fixk".to_owned(),
            generation: 1,
            target_adopt_generation: 1,
            plan_sha256: "cf9845c5c257a728cd95e3125b5b2500c0724980babd25df0ad32e5e046773ea".to_owned(),
            rows: 8000,
            tables: Vec::new(),
        };
        let mapping = MappingRead {
            config_digest: "b9bb844bd11ab53d92c42baac53c954cde6df785e1054b14b0bdfc3e1d15a88e".to_owned(),
            version: "17.10".to_owned(),
            role_state: "absent".to_owned(),
            tables: vec![LiveTable {
                logical_schema: "public".to_owned(),
                logical_table: "batch_b".to_owned(),
                physical_schema: "_oo".to_owned(),
                physical_table: "pt_b".to_owned(),
                rows: 8000,
                columns: vec![
                    LiveColumn { name: "id".to_owned(), physical_name: "id".to_owned(), type_name: "int8".to_owned(), typmod: -1, type_oid: 20 },
                    LiveColumn { name: "at_time".to_owned(), physical_name: "at_time".to_owned(), type_name: "time".to_owned(), typmod: -1, type_oid: 1083 },
                    LiveColumn { name: "blob".to_owned(), physical_name: "blob".to_owned(), type_name: "bytea".to_owned(), typmod: -1, type_oid: 17 },
                ],
            }],
        };
        write_dev_policy(&config, &sealed, &mapping).expect("policy");
        let document: Value = serde_json::from_slice(&fs::read(dir.join("work/dev-policy.json")).expect("read")).expect("json");
        assert_eq!(document["identity"]["role"], "oo_r_devread");
        assert_eq!(document["identity"]["policy_id"], mapping.config_digest);
        assert_eq!(document["policies"][0]["role"], "devread");
        assert_eq!(document["policies"][0]["login"], "oo_mirror_read");
        assert_eq!(document["policies"][0]["table"]["columns"][0]["status"], "auto_accepted");
        assert_eq!(document["policies"][0]["table"]["columns"].as_array().expect("columns").len(), 1);
        assert_eq!(document["catalog"]["max_functions"], 8);
        assert!(document.get("version").is_none());
        assert!(document.get("statement").is_none());
        assert_no_secret_key(document.as_object().expect("object"));
        let fact: Value = serde_json::from_slice(&fs::read(dir.join("work/dev-policy-fact.json")).expect("fact")).expect("fact json");
        assert_eq!(fact["publication"], "not_published");
        assert_eq!(fact["human_review"], false);
        assert_eq!(fact["role_state"], "absent");
        assert_eq!(fact["declared_foreign_key"], "not_declared");
        assert_eq!(fact["omitted"][1]["column"], "blob");
        let _ = fs::remove_dir_all(&dir);
    }

    fn assert_no_secret_key(map: &Map<String, Value>) {
        for (key, value) in map {
            assert_ne!(key, "password");
            assert_ne!(key, "bearer");
            assert_ne!(key, "token");
            if let Some(child) = value.as_object() {
                assert_no_secret_key(child);
            }
            if let Some(items) = value.as_array() {
                for item in items {
                    if let Some(child) = item.as_object() {
                        assert_no_secret_key(child);
                    }
                }
            }
        }
    }
}
