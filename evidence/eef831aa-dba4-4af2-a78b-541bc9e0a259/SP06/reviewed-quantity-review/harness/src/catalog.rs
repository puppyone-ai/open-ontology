//! Synthetic catalog and reviewed-policy operator documents.
//! The integer extremes are handwritten decimals.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::proc::{self, Stop};

pub const AMOUNT_I2: [&str; 3] = ["-32768", "32767", "0"];
pub const AMOUNT_I4: [&str; 3] = ["-2147483648", "2147483647", "0"];
pub const AMOUNT_I8: [&str; 3] = ["-9223372036854775808", "9223372036854775807", "0"];
const PROTOCOL: &str = "oo.gateway.reviewed-policy-cli/v1";
const CAPTURED_AT: &str = "2026-10-06T08:00:00.000000Z";
const TABLE_OWNER: &str = "lot_owner";
const LEAK: &str = "a259_owner_do_not_leak";
const LEAK_SHA256: &str = "ccbfa4cbbf7485e2cf465fe8a116f12f080acf1272f81b0ba2317319f5576811";

pub struct Col {
    pub attnum: i32,
    pub name: &'static str,
    pub oid: i64,
    pub type_name: &'static str,
}

pub struct TableSpec {
    pub schema: &'static str,
    pub name: &'static str,
    pub columns: Vec<Col>,
    pub pk: Vec<(i32, &'static str)>,
}

pub struct Identity {
    pub project: &'static str,
    pub source_id: &'static str,
    pub database: &'static str,
    pub digest: String,
    pub generation: i64,
    pub role: &'static str,
    pub login: &'static str,
}

pub struct Chain {
    pub artifact_json: String,
    pub command_json: String,
    pub decisions_json: String,
    pub schema_hash: String,
    pub artifact_sha256: String,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ch if (ch as u32) < 0x20 => {
                let code = ch as u32;
                out.push_str(&format!("\\u{code:04x}"));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn write_pair(buf: &mut String, name: &str, value: &str) {
    push_string(buf, name);
    buf.push(':');
    push_string(buf, value);
}

pub fn schema_preimage(table: &TableSpec) -> String {
    let mut buf = String::from("{\"tables\":[{");
    write_pair(&mut buf, "schema", table.schema);
    buf.push(',');
    write_pair(&mut buf, "name", table.name);
    buf.push_str(",\"relkind\":\"r\",\"replica_identity\":\"d\",\"rls\":false,\"force_rls\":false,\"columns\":[");
    for (index, column) in table.columns.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push_str("{\"attnum\":");
        buf.push_str(&column.attnum.to_string());
        buf.push_str(",\"name\":");
        push_string(&mut buf, column.name);
        buf.push_str(",\"type_oid\":");
        buf.push_str(&column.oid.to_string());
        buf.push_str(",\"typmod\":-1,\"not_null\":true,\"collation_oid\":0,\"typtype\":\"b\",\"base_type_oid\":null,\"identity\":\"\",\"generated\":\"\",\"default_sha256\":null,\"enum_labels\":[],\"domain_checks\":[]}");
    }
    buf.push_str("],\"primary_key\":[");
    for (index, (attnum, name)) in table.pk.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push_str("{\"attnum\":");
        buf.push_str(&attnum.to_string());
        buf.push_str(",\"name\":");
        push_string(&mut buf, name);
        buf.push('}');
    }
    buf.push_str("],\"primary_key_nulls_not_distinct\":false,\"unique_keys\":[],\"foreign_keys\":[],\"policies\":[]}]}");
    buf
}

fn column_json(column: &Col) -> Value {
    let reason = if column.type_name == "numeric" {
        "unbounded_or_wide_numeric"
    } else {
        "known_builtin_not_converted"
    };
    json!({
        "attnum": column.attnum,
        "name": column.name,
        "name_ident": column.name,
        "type_oid": column.oid,
        "type_schema": "pg_catalog",
        "type_name": column.type_name,
        "typtype": "b",
        "typmod": -1,
        "base_type_oid": Value::Null,
        "numeric_precision": Value::Null,
        "numeric_scale": Value::Null,
        "not_null": true,
        "collation_oid": 0,
        "collation_name": Value::Null,
        "identity": "",
        "generated": "",
        "default_expr": Value::Null,
        "comment": Value::Null,
        "enum_labels": [],
        "domain_checks": [],
        "type_support": {"converted": false, "reason": reason},
        "current_privileges": {"select": true, "insert": false, "update": false}
    })
}

pub fn catalog_json(table: &TableSpec, identity: &Identity) -> Value {
    let hash = sha256_hex(schema_preimage(table).as_bytes());
    json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": identity.source_id,
        "database": identity.database,
        "captured_at": CAPTURED_AT,
        "schema_hash": hash,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": [json!({
            "schema": table.schema,
            "schema_ident": table.schema,
            "name": table.name,
            "name_ident": table.name,
            "relkind": "r",
            "replica_identity": "d",
            "rls_enabled": false,
            "rls_forced": false,
            "owner": TABLE_OWNER,
            "comment": {"byte_len": 22, "sha256": LEAK_SHA256},
            "columns": table.columns.iter().map(column_json).collect::<Vec<_>>(),
            "primary_key": table.pk.iter().map(|(attnum, name)| json!({"attnum": attnum, "name": name})).collect::<Vec<_>>(),
            "primary_key_nulls_not_distinct": false,
            "unique_keys": [],
            "foreign_keys": [],
            "policies": [],
            "privileges": [{"grantee": TABLE_OWNER, "privilege": "SELECT", "grantable": false}]
        })],
        "type_conversion": "unsupported"
    })
}

pub fn settings(identity: &Identity, schema: &str) -> Value {
    json!({
        "project": identity.project,
        "ontology_version": "ont-a259",
        "catalog_version": "cat-a259",
        "policy": {"id": "pol-a259", "version": "1"},
        "knowledge": {"id": "know-a259", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": identity.project,
            "locale": "en-US",
            "include_schemas": [schema],
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
    })
}

pub fn fingerprints_of(artifact: &Value) -> Result<BTreeMap<(String, String), String>, Stop> {
    let mut out = BTreeMap::new();
    let queue = artifact["draft"]["review_queue"]
        .as_array()
        .ok_or_else(|| Stop::new("captured-pg-draft", "review queue"))?;
    for item in queue {
        if item["kind"] != "structural_column" {
            continue;
        }
        let item_id = item["item_id"]
            .as_str()
            .ok_or_else(|| Stop::new("captured-pg-draft", "item id"))?;
        let Some(rest) = item_id.strip_prefix("column/") else {
            continue;
        };
        let Some((dataset, column)) = rest.rsplit_once('/') else {
            continue;
        };
        let fingerprint = item["fact_fingerprint"]
            .as_str()
            .ok_or_else(|| Stop::new("captured-pg-draft", "fingerprint"))?
            .to_string();
        out.insert((dataset.to_string(), column.to_string()), fingerprint);
    }
    Ok(out)
}

pub fn decision(
    fingerprints: &BTreeMap<(String, String), String>,
    dataset: &str,
    schema_hash: &str,
    column: &str,
    pii: &str,
    semantic: &str,
) -> Result<Value, Stop> {
    let fingerprint = fingerprints
        .get(&(dataset.to_string(), column.to_string()))
        .ok_or_else(|| Stop::new("decision", format!("missing fingerprint {dataset}.{column}")))?
        .clone();
    Ok(json!({
        "dataset": dataset,
        "column": column,
        "schema_hash": schema_hash,
        "fact_fingerprint": fingerprint,
        "pii": pii,
        "semantic_type": semantic,
        "reviewer": {"by": "nea.reviewer", "at": "2026-10-06"}
    }))
}

pub fn compile_table(
    table: &TableSpec,
    identity: &Identity,
    decisions: Vec<Value>,
) -> Result<Chain, Stop> {
    let catalog = catalog_json(table, identity);
    let artifact_json = proc::run_ok(
        "OO_A259_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).map_err(|_| Stop::new("catalog", "encode"))?,
    )?;
    if artifact_json.ends_with('\n') {
        return Err(Stop::new("captured-pg-draft", "trailing newline"));
    }
    if artifact_json.contains(LEAK) {
        return Err(Stop::new("captured-pg-draft", "comment body leaked"));
    }
    let artifact: Value = serde_json::from_str(&artifact_json).map_err(|_| Stop::new("captured-pg-draft", "json"))?;
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .ok_or_else(|| Stop::new("captured-pg-draft", "schema hash"))?
        .to_string();
    if schema_hash != catalog["schema_hash"].as_str().unwrap_or("") {
        return Err(Stop::new("captured-pg-draft", "schema hash mismatch"));
    }
    let decisions_json = serde_json::to_string(&decisions).map_err(|_| Stop::new("decisions", "encode"))?;
    let command_in = json!({
        "protocol": "oo.builder.reviewed-effective-oos-command/v1",
        "artifact": artifact,
        "settings": settings(identity, table.schema),
        "decisions": decisions,
    });
    let command_json = proc::run_ok(
        "OO_A259_REVIEWED_EFFECTIVE",
        &[],
        &serde_json::to_string(&command_in).map_err(|_| Stop::new("reviewed-effective", "encode"))?,
    )?;
    if command_json.ends_with('\n') {
        return Err(Stop::new("reviewed-effective", "trailing newline"));
    }
    if command_json.contains(LEAK) {
        return Err(Stop::new("reviewed-effective", "comment body leaked"));
    }
    let command: Value = serde_json::from_str(&command_json).map_err(|_| Stop::new("reviewed-effective", "json"))?;
    let artifact_sha256 = command["provenance"]["sealed_source_sha256"]
        .as_str()
        .ok_or_else(|| Stop::new("reviewed-effective", "sealed hash"))?
        .to_string();
    Ok(Chain {
        artifact_json,
        command_json,
        decisions_json,
        schema_hash,
        artifact_sha256,
    })
}

pub fn operator_for(
    chain: &Chain,
    identity: &Identity,
    dataset: &str,
    allow: &[(&str, bool)],
) -> Result<String, Stop> {
    let allow_json: Vec<Value> = allow
        .iter()
        .map(|(column, explicit)| {
            json!({
                "dataset": dataset,
                "column": column,
                "explicit_allow": explicit
            })
        })
        .collect();
    serde_json::to_string(&json!({
        "protocol": PROTOCOL,
        "command": chain.command_json,
        "artifact": chain.artifact_json,
        "decisions": chain.decisions_json,
        "binding": {
            "identity": {
                "project": identity.project,
                "ontology_version": "ont-a259",
                "catalog_version": "cat-a259",
                "policy_id": "pol-a259",
                "policy_version": "1",
                "knowledge_id": "know-a259",
                "knowledge_version": "1"
            },
            "seal": {
                "source_id": identity.source_id,
                "schema": dataset.split_once('.').map(|(schema, _)| schema).unwrap_or(""),
                "schema_hash": chain.schema_hash,
                "artifact_sha256": chain.artifact_sha256
            },
            "config_digest": identity.digest,
            "generation": identity.generation,
            "role": identity.role,
            "login": identity.login,
            "allow": allow_json
        }
    }))
    .map_err(|_| Stop::new("reviewed-policy", "encode"))
}

pub fn run_policy(stdin_text: &str) -> Result<proc::Spawn, Stop> {
    let program = proc::bin("OO_A259_REVIEWED_POLICY")?;
    proc::spawn(&program, &[], stdin_text.as_bytes())
}

pub fn parse_policies(stdout: &str) -> Result<Vec<oo_gateway::ColumnAclPolicy>, Stop> {
    if stdout.is_empty() || stdout.ends_with('\n') {
        return Err(Stop::new("reviewed-policy", "stdout shape"));
    }
    if stdout.contains(LEAK) {
        return Err(Stop::new("reviewed-policy", "comment body leaked"));
    }
    let doc: Value = serde_json::from_str(stdout).map_err(|_| Stop::new("reviewed-policy", "json"))?;
    if doc["protocol"] != PROTOCOL {
        return Err(Stop::new("reviewed-policy", "protocol"));
    }
    let policies = doc["policies"]
        .as_array()
        .ok_or_else(|| Stop::new("reviewed-policy", "policies"))?;
    let mut out = Vec::with_capacity(policies.len());
    for item in policies {
        out.push(policy_from(item)?);
    }
    Ok(out)
}

fn policy_from(item: &Value) -> Result<oo_gateway::ColumnAclPolicy, Stop> {
    let columns = item["table"]["columns"]
        .as_array()
        .ok_or_else(|| Stop::new("reviewed-policy", "columns"))?;
    let mut specs = Vec::with_capacity(columns.len());
    for column in columns {
        specs.push(oo_gateway::ColumnSpec {
            name: text(column, "name")?,
            status: match column["status"].as_str() {
                Some("confirmed") => oo_gateway::ClassStatus::Confirmed,
                _ => return Err(Stop::new("reviewed-policy", "status")),
            },
            semantic_type: text(column, "semantic_type")?,
            pii: pii_from(&column["pii"])?,
        });
    }
    let row_filter = if item["row_filter"].is_null() {
        None
    } else {
        return Err(Stop::new("reviewed-policy", "row_filter"));
    };
    Ok(oo_gateway::ColumnAclPolicy {
        project: text(item, "project")?,
        config_digest: text(item, "config_digest")?,
        generation: item["generation"]
            .as_i64()
            .ok_or_else(|| Stop::new("reviewed-policy", "generation"))?,
        role: text(item, "role")?,
        login: text(item, "login")?,
        table: oo_gateway::LogicalTable {
            namespace: text(&item["table"], "namespace")?,
            name: text(&item["table"], "name")?,
            columns: specs,
        },
        row_filter,
    })
}

fn pii_from(pii: &Value) -> Result<oo_gateway::PiiClass, Stop> {
    match pii["class"].as_str() {
        Some("none") if pii.get("explicit_allow").is_none() => Ok(oo_gateway::PiiClass::None),
        Some("declared") if pii["explicit_allow"] == true => {
            Ok(oo_gateway::PiiClass::Declared { explicit_allow: true })
        }
        _ => Err(Stop::new("reviewed-policy", "pii")),
    }
}

fn text(value: &Value, key: &str) -> Result<String, Stop> {
    value[key]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Stop::new("reviewed-policy", key))
}

pub fn column<'a>(
    policy: &'a oo_gateway::ColumnAclPolicy,
    name: &str,
) -> Result<&'a oo_gateway::ColumnSpec, Stop> {
    policy
        .table
        .columns
        .iter()
        .find(|column| column.name == name)
        .ok_or_else(|| Stop::new("reviewed-policy", format!("missing {name}")))
}
