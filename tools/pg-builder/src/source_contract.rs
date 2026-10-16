//! Project sealed column caveats onto the role pack.
//!
//! Core `caveat_agrees` still decides which tokens a sealed catalog can store.
//! This module does not rewrite a sealed catalog and does not invent a grade.

use serde_json::{Map, Value, json};

use oo_builder::{compile_role_pack, compile_sealed_import_catalog, compile_sealed_import_collection, SEALED_IMPORT_COLLECTION_PROTOCOL};
use oo_core::source_bound::{self, DerivedSourceBound};

use crate::PgBuilderError;

/// Pack command for one sealed catalog plus one source contract.
pub const PROTOCOL: &str = "oo.pg-builder.bind-source-pack/v1";
const CONTRACT_SCHEMA: &str = "oo.pg-builder.source-contract/v1";

const CLOSED_CAVEATS: &[&str] = &[
    "bigint_unsigned_numeric",
    "empty_string_is_null",
    "enum_as_text",
    "json_not_jsonb",
    "lossy_type_fallback",
    "mysql_time_not_interval",
    "offset_lost",
    "precision_loss",
    "source_collation_ci",
    "tinyint1_as_int2",
    "trailing_space_insensitive",
    "unconstrained_number",
];

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SealedCaveat {
    dataset: String,
    column: String,
    token: String,
}

/// Copy sealed caveats onto `effective` when the contract names the same tokens.
///
/// A `oo.source-semantics.source-bound/v1` observation is not a storage caveat.
/// It is derived beside the sealed compiler. Grade and delete detection stay
/// absent when the observation did not record them. `unconstrained_number`
/// stays a native note.
pub fn project_sealed_source_contract(
    effective: &mut Value,
    catalog: &Value,
    contract: Option<&Value>,
) -> Result<(), PgBuilderError> {
    if let Some(bound) = contract.filter(|value| {
        value.get("protocol").and_then(Value::as_str) == Some(source_bound::SOURCE_BOUND_PROTOCOL)
    }) {
        return project_derived_source_bound(effective, catalog, bound);
    }
    let caveats = sealed_caveats(catalog)?;
    if caveats.is_empty() {
        if contract.is_some() {
            return Err(rejected());
        }
        return Ok(());
    }
    let Some(contract) = contract else {
        return Err(missing());
    };
    if contract.get("schema").and_then(Value::as_str) != Some(CONTRACT_SCHEMA) {
        return Err(rejected());
    }
    let engine = sealed_engine(catalog)?;
    if !source_name(engine) || !contract_matches(contract, engine, &caveats)? {
        return Err(rejected());
    }
    let datasets = effective
        .get_mut("datasets")
        .and_then(Value::as_array_mut)
        .ok_or_else(rejected)?;
    let mut pending = caveats;
    for dataset in datasets.iter_mut() {
        let name = dataset.get("dataset").and_then(Value::as_str).unwrap_or("");
        let mut rows = Vec::new();
        pending.retain(|item| {
            if item.dataset == name {
                rows.push(item.clone());
                false
            } else {
                true
            }
        });
        if rows.is_empty() {
            continue;
        }
        let object = dataset.as_object_mut().ok_or_else(rejected)?;
        object.insert("source_semantics".to_string(), semantics(engine, contract, &rows));
    }
    if pending.is_empty() {
        Ok(())
    } else {
        Err(rejected())
    }
}

/// Project receipt-bound Doris methods into OOS, initializing missing source
/// identity from the authenticated observation and sealed engine. Conflicts fail closed.
pub fn project_doris_capture_methods(
    effective: &mut Value,
    catalog: &Value,
    observation: &Value,
) -> Result<(), PgBuilderError> {
    const PROTOCOL: &str = "oo.source.doris.audit/v1";
    let engine = sealed_engine(catalog)?;
    if observation.get("protocol").and_then(Value::as_str) != Some(PROTOCOL)
        || observation.get("source_id").and_then(Value::as_str) != Some("dorispreview")
        || observation.get("product").and_then(Value::as_str) != Some("Apache Doris")
        || engine != "doris"
    {
        return Err(rejected());
    }
    let version = observation.get("version").and_then(Value::as_str)
        .filter(|value| version_token(value)).ok_or_else(rejected)?;
    let entries = catalog
        .get("entries")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .ok_or_else(rejected)?;
    let methods = observation
        .get("capture_methods")
        .and_then(Value::as_object)
        .ok_or_else(rejected)?;
    let generations = observation
        .get("table_generations")
        .and_then(Value::as_object)
        .ok_or_else(rejected)?;
    let mut by_dataset = std::collections::BTreeMap::new();
    for entry in entries {
        let tables = entry.get("tables").and_then(Value::as_array).filter(|rows| !rows.is_empty()).ok_or_else(rejected)?;
        let source_generation = entry.get("source_generation").and_then(Value::as_i64).ok_or_else(rejected)?;
        let target_generation = entry.get("target_generation").and_then(Value::as_i64).ok_or_else(rejected)?;
        if entry.get("source_id").and_then(Value::as_str) != observation.get("source_id").and_then(Value::as_str) {
            return Err(rejected());
        }
        for table in tables {
            let schema = table.get("logical_schema").and_then(Value::as_str).ok_or_else(rejected)?;
            let table_name = table.get("logical_table").and_then(Value::as_str).ok_or_else(rejected)?;
            let dataset = format!("{schema}.{table_name}");
            if by_dataset.insert(dataset, (source_generation, target_generation)).is_some() {
                return Err(rejected());
            }
        }
    }
    if methods.len() != by_dataset.len() || generations.len() != by_dataset.len() {
        return Err(rejected());
    }
    let datasets = effective.get_mut("datasets").and_then(Value::as_array_mut).ok_or_else(rejected)?;
    if datasets.len() != by_dataset.len() {
        return Err(rejected());
    }
    let mut seen = std::collections::BTreeSet::new();
    for dataset in datasets {
        let name = dataset.get("dataset").and_then(Value::as_str).ok_or_else(rejected)?;
        let (source_generation, target_generation) = by_dataset.get(name).ok_or_else(rejected)?;
        if !seen.insert(name.to_owned()) {
            return Err(rejected());
        }
        let method = methods.get(name).and_then(Value::as_object).ok_or_else(rejected)?;
        let generation = generations.get(name).and_then(Value::as_object).ok_or_else(rejected)?;
        let capture = method.get("capture_method").and_then(Value::as_str).ok_or_else(rejected)?;
        if method.get("logical_table").and_then(Value::as_str) != Some(name)
            || !matches!(capture, "c1_cdc" | "c2_change_feed" | "c3_cursor" | "c4_partition" | "c5_diff")
            || method.get("consistency").and_then(Value::as_str) != Some("S0")
            || method.get("source_generation").and_then(Value::as_i64) != Some(*source_generation)
            || method.get("target_generation").and_then(Value::as_i64) != Some(*target_generation)
            || generation.get("source_generation").and_then(Value::as_i64) != Some(*source_generation)
            || generation.get("target_generation").and_then(Value::as_i64) != Some(*target_generation)
        {
            return Err(rejected());
        }
        let deletion = method.get("delete_detection").and_then(Value::as_str).ok_or_else(rejected)?;
        if !matches!(deletion, "none" | "reconcile" | "partition_replace")
            || (capture == "c4_partition" && deletion != "partition_replace")
            || (capture == "c5_diff" && !matches!(deletion, "reconcile" | "none"))
            || (!matches!(capture, "c4_partition" | "c5_diff") && deletion != "none")
        {
            return Err(rejected());
        }
        let current_update = method.get("current_update").and_then(Value::as_str);
        if current_update.is_some_and(|value| !matches!(value, "observed" | "none_observed"))
            || (current_update == Some("none_observed") && capture != "c5_diff")
            || (capture == "c5_diff" && deletion == "none" && current_update != Some("none_observed"))
        {
            return Err(rejected());
        }
        let object = dataset.as_object_mut().ok_or_else(rejected)?;
        let semantics = object.entry("source_semantics").or_insert_with(|| json!({}))
            .as_object_mut().ok_or_else(rejected)?;
        for (key, expected) in [
            ("family", json!(engine)),
            ("product", json!(engine)),
            ("version", json!(version)),
            ("source_kind", json!("database")),
            ("capture", json!(capture)),
            ("grade", json!("S0")),
            ("delete_detection", json!(deletion)),
        ] {
            match semantics.get(key) {
                Some(actual) if actual == &expected => {}
                // Sealed import is a compiler marker, not an observed source method.
                Some(Value::String(value)) if key == "capture" && value == "sealed_import" => {
                    semantics.insert(key.to_owned(), expected);
                }
                Some(Value::Null) | None => { semantics.insert(key.to_owned(), expected); }
                Some(_) => return Err(rejected()),
            }
        }
        serde_json::from_value::<oo_core::oos::SourceSemantics>(Value::Object(semantics.clone()))
            .map_err(|_| rejected())?;

    }
    Ok(())
}

/// Project only a validated transactional MySQL cut onto the standard OOS fields.
pub fn project_mysql_c1(effective: &mut Value, catalog: &Value, observation: &Value) -> Result<(), PgBuilderError> {
    if sealed_engine(catalog)? != "mysql" || observation["protocol"] != "oo.source.mysql.audit/v1"
        || observation["capture_method"] != "C1" || observation["consistency_grade"] != "T"
        || observation["delete_detection"] != "native" || observation["cross_table_consistent"] != true
    { return Err(rejected()); }
    let entries = catalog["entries"].as_array().ok_or_else(rejected)?;
    let mut expected = std::collections::BTreeMap::new();
    for entry in entries {
        if entry["source_id"] != observation["source_id"] || entry["source_generation"] != observation["source_generation"]
            || entry["target_generation"] != observation["target_generation"] { return Err(rejected()); }
        for table in entry["tables"].as_array().ok_or_else(rejected)? {
            let schema = table["logical_schema"].as_str().ok_or_else(rejected)?;
            let name = table["logical_table"].as_str().ok_or_else(rejected)?;
            let columns = table["columns"].as_array().ok_or_else(rejected)?;
            let rejects = table["rejects"].as_array().ok_or_else(rejected)?;
            let mut zero_dates = std::collections::BTreeSet::new();
            for reject in rejects.iter().filter(|row| row["reason"] == "zero_date") {
                let column = reject["column"].as_str().ok_or_else(rejected)?;
                let (index, fact) = columns.iter().enumerate().find(|(_, fact)| fact["logical_name"] == column).ok_or_else(rejected)?;
                if reject["attnum"].as_u64() != Some(index as u64 + 1)
                    || !matches!((fact["source_type"].as_str(), fact["oid"].as_u64()),
                        (Some("date32"), Some(1082)) | (Some("timestamp_micros"), Some(1114)))
                { return Err(rejected()); }
                zero_dates.insert(column);
            }
            let mut notes = Vec::new();
            for column in columns {
                let name = column["logical_name"].as_str().ok_or_else(rejected)?;
                let mut caveats = Vec::new();
                if let Some(token) = column["caveat"].as_str() {
                    if !CLOSED_CAVEATS.contains(&token) { return Err(rejected()); }
                    caveats.push(token);
                }
                let note = if zero_dates.contains(name) {
                    caveats.push("mysql_zero_date_to_null");
                    Some("mysql_zero_date_to_null_authenticated_reject")
                } else if column["source_type"] == "duration_micros" {
                    if column["oid"] != 1186 { return Err(rejected()); }
                    Some("mysql_signed_time_duration_to_interval")
                } else { None };
                if note.is_some() || !caveats.is_empty() {
                    notes.push(json!({"column":name,"caveats":caveats,"native_note":note}));
                }
            }
            let semantics = json!({"family":"mysql","product":"mysql","version":observation["version"],
                "capture":"c1_cdc","grade":"T","source_kind":"database","delete_detection":"native","columns":notes});
            serde_json::from_value::<oo_core::oos::SourceSemantics>(semantics.clone()).map_err(|_| rejected())?;
            if expected.insert(format!("{schema}.{name}"), semantics).is_some() { return Err(rejected()); }
        }
    }
    let datasets = effective["datasets"].as_array_mut().ok_or_else(rejected)?;
    if datasets.len() != expected.len() { return Err(rejected()); }
    for dataset in datasets {
        let name = dataset["dataset"].as_str().ok_or_else(rejected)?;
        let semantics = expected.remove(name).ok_or_else(rejected)?;
        dataset["source_semantics"] = semantics;
    }
    Ok(())
}

fn sealed_engine(catalog: &Value) -> Result<&str, PgBuilderError> {
    if catalog.get("protocol").and_then(Value::as_str) != Some(SEALED_IMPORT_COLLECTION_PROTOCOL) {
        return catalog.get("engine").and_then(Value::as_str)
            .ok_or_else(|| PgBuilderError::new("input.schema", "source catalog engine is missing"));
    }
    let entries = catalog.get("entries").and_then(Value::as_array)
        .filter(|entries| !entries.is_empty()).ok_or_else(rejected)?;
    let engine = entries[0].get("engine").and_then(Value::as_str).ok_or_else(rejected)?;
    if entries.iter().any(|entry| entry.get("engine").and_then(Value::as_str) != Some(engine)) {
        return Err(rejected());
    }
    Ok(engine)
}

/// Project, require a compiler-accepted catalog, then compile the role pack.
pub fn bind_source_pack(
    catalog_json: &str,
    effective_json: &str,
    visibility_json: &str,
    contract_json: Option<&str>,
) -> Result<Value, PgBuilderError> {
    let catalog: Value = serde_json::from_str(catalog_json)
        .map_err(|_| PgBuilderError::new("input.schema", "source pack catalog is not JSON"))?;
    let mut effective: Value = serde_json::from_str(effective_json)
        .map_err(|_| PgBuilderError::new("input.schema", "source pack effective document is not JSON"))?;
    let contract = match contract_json {
        Some(text) => Some(
            serde_json::from_str::<Value>(text)
                .map_err(|_| PgBuilderError::new("input.schema", "source contract is not JSON"))?,
        ),
        None => None,
    };
    project_sealed_source_contract(&mut effective, &catalog, contract.as_ref())?;
    if catalog.get("protocol").and_then(Value::as_str) == Some(SEALED_IMPORT_COLLECTION_PROTOCOL) {
        compile_sealed_import_collection(catalog_json)
            .map_err(|err| PgBuilderError::forwarded(err.code, err.message))?;
    } else {
        compile_sealed_import_catalog(catalog_json)
            .map_err(|err| PgBuilderError::forwarded(err.code, err.message))?;
    }
    let effective_text = serde_json::to_string(&effective)
        .map_err(|_| PgBuilderError::new("input.schema", "source pack effective document is not JSON"))?;
    let pack = compile_role_pack(&effective_text, visibility_json)
        .map_err(|err| PgBuilderError::forwarded(err.code, err.message))?;
    Ok(json!({
        "protocol": PROTOCOL,
        "outcome": "compiled",
        "ready": false,
        "partial_ready": false,
        "pack_digest": pack.pack_digest,
        "artifact": pack.artifact,
    }))
}

fn project_derived_source_bound(
    effective: &mut Value,
    catalog: &Value,
    bound: &Value,
) -> Result<(), PgBuilderError> {
    let catalog_json = serde_json::to_string(catalog)
        .map_err(|_| PgBuilderError::new("input.schema", "source pack catalog is not JSON"))?;
    let bound_json = serde_json::to_string(bound)
        .map_err(|_| PgBuilderError::new("input.schema", "source-bound observation is not JSON"))?;
    let derived = source_bound::derive_source_bound(&catalog_json, &bound_json).map_err(|err| {
        PgBuilderError::forwarded(err.code, err.message)
    })?;
    let dataset_name = derived
        .columns
        .first()
        .map(|column| column.dataset.as_str())
        .ok_or_else(|| PgBuilderError::new("schema.invalid", "source-bound derivation has no column"))?;
    let datasets = effective
        .get_mut("datasets")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| PgBuilderError::new("input.schema", "effective document has no datasets"))?;
    let mut placed = false;
    for dataset in datasets.iter_mut() {
        if dataset.get("dataset").and_then(Value::as_str) != Some(dataset_name) {
            continue;
        }
        let object = dataset.as_object_mut().ok_or_else(|| {
            PgBuilderError::new("input.schema", "effective dataset is not an object")
        })?;
        object.insert("source_semantics".to_string(), semantics_from_derived(&derived));
        placed = true;
    }
    if placed {
        Ok(())
    } else {
        Err(PgBuilderError::new(
            "schema.invalid",
            "source-bound dataset is not on the effective document",
        ))
    }
}

fn semantics_from_derived(derived: &DerivedSourceBound) -> Value {
    let mut columns = Vec::with_capacity(derived.columns.len());
    for column in &derived.columns {
        let mut row = Map::new();
        row.insert("column".to_string(), json!(column.logical_name));
        if !column.caveats.is_empty() {
            row.insert("caveats".to_string(), json!(column.caveats));
        }
        if let Some(note) = &column.native_note {
            row.insert("native_note".to_string(), json!(note));
        }
        columns.push(Value::Object(row));
    }
    let mut semantics = Map::new();
    semantics.insert("family".to_string(), json!(derived.family));
    semantics.insert("product".to_string(), json!(derived.product));
    semantics.insert("version".to_string(), json!(derived.version));
    semantics.insert("capture".to_string(), json!(derived.capture));
    semantics.insert("source_kind".to_string(), json!(derived.source_kind));
    if let Some(grade) = &derived.consistency_grade {
        semantics.insert("grade".to_string(), json!(grade));
    }
    if let Some(delete_detection) = &derived.delete_detection {
        semantics.insert("delete_detection".to_string(), json!(delete_detection));
    }
    semantics.insert("columns".to_string(), Value::Array(columns));
    Value::Object(semantics)
}

fn semantics(engine: &str, contract: &Value, rows: &[SealedCaveat]) -> Value {
    json!({
        "capture": "sealed_import",
        "columns": rows.iter().map(|row| json!({
            "caveats": [row.token.clone()],
            "column": row.column,
        })).collect::<Vec<_>>(),
        "delete_detection": contract.get("delete_detection").cloned().unwrap_or(Value::Null),
        "family": engine,
        "grade": contract.get("grade").cloned().unwrap_or(Value::Null),
        "product": engine,
        "source_kind": contract.get("source_kind").cloned().unwrap_or(Value::Null),
        "version": contract.get("version").cloned().unwrap_or(Value::Null),
    })
}

fn sealed_caveats(catalog: &Value) -> Result<Vec<SealedCaveat>, PgBuilderError> {
    if catalog.get("protocol").and_then(Value::as_str) == Some(SEALED_IMPORT_COLLECTION_PROTOCOL) {
        let entries = catalog.get("entries").and_then(Value::as_array)
            .ok_or_else(|| PgBuilderError::new("input.schema", "source collection entries are missing"))?;
        let mut all = Vec::new();
        for entry in entries { all.extend(sealed_caveats(entry)?); }
        return Ok(all);
    }
    let tables = catalog
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("input.schema", "source catalog tables are missing"))?;
    let mut out = Vec::new();
    for table in tables {
        let schema = table.get("logical_schema").and_then(Value::as_str).unwrap_or("");
        let name = table.get("logical_table").and_then(Value::as_str).unwrap_or("");
        if schema.is_empty() || name.is_empty() {
            return Err(PgBuilderError::new("input.schema", "source catalog table name is missing"));
        }
        let dataset = format!("{schema}.{name}");
        let columns = table
            .get("columns")
            .and_then(Value::as_array)
            .ok_or_else(|| PgBuilderError::new("input.schema", "source catalog columns are missing"))?;
        for column in columns {
            let token = match column.get("caveat") {
                None | Some(Value::Null) => continue,
                Some(Value::String(text)) => text.as_str(),
                Some(_) => return Err(rejected()),
            };
            if !CLOSED_CAVEATS.contains(&token) {
                return Err(rejected());
            }
            let logical = column.get("logical_name").and_then(Value::as_str).ok_or_else(rejected)?;
            if !table_name(logical) {
                return Err(rejected());
            }
            out.push(SealedCaveat {
                dataset: dataset.clone(),
                column: logical.to_string(),
                token: token.to_string(),
            });
        }
    }
    out.sort();
    if out.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(rejected());
    }
    Ok(out)
}

fn contract_matches(contract: &Value, engine: &str, sealed: &[SealedCaveat]) -> Result<bool, PgBuilderError> {
    let family = contract.get("family").and_then(Value::as_str).unwrap_or("");
    let product = contract.get("product").and_then(Value::as_str).unwrap_or("");
    let version = contract.get("version").and_then(Value::as_str).unwrap_or("");
    let capture = contract.get("capture").and_then(Value::as_str).unwrap_or("");
    let grade = contract.get("grade").and_then(Value::as_str).unwrap_or("");
    let source_kind = contract.get("source_kind").and_then(Value::as_str).unwrap_or("");
    let delete_detection = contract.get("delete_detection").and_then(Value::as_str).unwrap_or("");
    if family != engine
        || product != engine
        || capture != "sealed_import"
        || !version_token(version)
        || !matches!(grade, "T" | "S1" | "S0" | "B")
        || !source_name(source_kind)
        || !source_name(delete_detection)
    {
        return Ok(false);
    }
    let rows = contract
        .get("columns")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("input.schema", "source contract columns are missing"))?;
    let mut got = Vec::with_capacity(rows.len());
    for row in rows {
        let dataset = row.get("dataset").and_then(Value::as_str).ok_or_else(rejected)?;
        let column = row.get("column").and_then(Value::as_str).ok_or_else(rejected)?;
        let token = match row.get("caveat") {
            Some(Value::String(text)) => text.as_str(),
            _ => return Err(rejected()),
        };
        if row.get("caveats").is_some() || !table_name(column) || !dataset_name(dataset) {
            return Err(rejected());
        }
        got.push(SealedCaveat {
            dataset: dataset.to_string(),
            column: column.to_string(),
            token: token.to_string(),
        });
    }
    got.sort();
    Ok(got == sealed)
}

fn dataset_name(value: &str) -> bool {
    let Some((schema, table)) = value.split_once('.') else {
        return false;
    };
    !schema.is_empty()
        && !table.is_empty()
        && !schema.contains('.')
        && !table.contains('.')
        && table_name(schema)
        && table_name(table)
}

fn source_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn table_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn version_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'+' | b'-'))
}

fn rejected() -> PgBuilderError {
    PgBuilderError::new("caveat.rejected", "the sealed caveat does not match the source contract")
}

fn missing() -> PgBuilderError {
    PgBuilderError::new("caveat.missing_contract", "a sealed caveat has no source contract")
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_builder::REVIEWED_SEALED_IMPORT_COMMAND_PROTOCOL;
    use oo_core::api::payload_sha256;
    use oo_core::sealed_import::{push_schema_column, push_schema_table};
    use serde_json::json;

    #[test]
    fn projects_only_generation_bound_doris_capture_quality() {
        let catalog = json!({"protocol":"oo.pipeline.sealed-import-collection/v1","entries":[
            {"engine":"doris","source_id":"dorispreview","source_generation":16,"target_generation":28,"tables":[{"logical_schema":"s","logical_table":"part_day"}]},
            {"engine":"doris","source_id":"dorispreview","source_generation":1,"target_generation":2,"tables":[{"logical_schema":"s","logical_table":"agg_qty"}]}
        ]});
        let observation = json!({"protocol":"oo.source.doris.audit/v1","source_id":"dorispreview","product":"Apache Doris","version":"doris-3.1.4-rc02-7f5ba43de6c","capture_methods":{
            "s.part_day":{"logical_table":"s.part_day","capture_method":"c4_partition","consistency":"S0","delete_detection":"partition_replace","source_generation":16,"target_generation":28},
            "s.agg_qty":{"logical_table":"s.agg_qty","capture_method":"c5_diff","consistency":"S0","delete_detection":"none","source_generation":1,"target_generation":2,"current_update":"none_observed"}
        },"table_generations":{
            "s.part_day":{"source_generation":16,"target_generation":28},
            "s.agg_qty":{"source_generation":1,"target_generation":2}
        }});
        let mut effective = json!({"datasets":[
            {"dataset":"s.part_day"},
            {"dataset":"s.agg_qty","source_semantics":{"family":"doris","product":"doris","version":"doris-3.1.4-rc02-7f5ba43de6c","source_kind":"database","capture":"sealed_import","grade":"S0","delete_detection":"none"}}
        ]});
        project_doris_capture_methods(&mut effective, &catalog, &observation).unwrap();
        assert_eq!(effective["datasets"][0]["source_semantics"]["capture"], "c4_partition");
        assert_eq!(effective["datasets"][1]["source_semantics"]["capture"], "c5_diff");
        assert_eq!(effective["datasets"][1]["source_semantics"]["delete_detection"], "none");
        assert_eq!(effective["datasets"][0]["source_semantics"]["family"], "doris");
        assert_eq!(effective["datasets"][0]["source_semantics"]["product"], "doris");
        assert_eq!(effective["datasets"][0]["source_semantics"]["version"], "doris-3.1.4-rc02-7f5ba43de6c");
        assert_eq!(effective["datasets"][0]["source_semantics"]["source_kind"], "database");
        assert_eq!(effective["datasets"][0]["source_semantics"]["grade"], "S0");
        for dataset in effective["datasets"].as_array().unwrap() {
            serde_json::from_value::<oo_core::oos::SourceSemantics>(dataset["source_semantics"].clone()).unwrap();
        }
    }

    #[test]
    fn rejects_doris_capture_method_identity_and_generation_drift() {
        let catalog = json!({"protocol":"oo.pipeline.sealed-import-collection/v1","entries":[
            {"engine":"doris","source_id":"dorispreview","source_generation":16,"target_generation":28,"tables":[{"logical_schema":"s","logical_table":"part_day"}]}
        ]});
        let base = json!({"protocol":"oo.source.doris.audit/v1","source_id":"dorispreview","product":"Apache Doris","version":"doris-3.1.4-rc02-7f5ba43de6c","capture_methods":{
            "s.part_day":{"logical_table":"s.part_day","capture_method":"c4_partition","consistency":"S0","delete_detection":"partition_replace","source_generation":16,"target_generation":28}
        },"table_generations":{"s.part_day":{"source_generation":16,"target_generation":28}}});
        let mut method_drift = base.clone();
        method_drift["capture_methods"]["s.part_day"]["capture_method"] = json!("static");
        let mut stale_generation = base.clone();
        stale_generation["capture_methods"]["s.part_day"]["target_generation"] = json!(27);
        for invalid in [method_drift, stale_generation] {
            let mut effective = json!({"datasets":[{"dataset":"s.part_day","source_semantics":{}}]});
            assert!(project_doris_capture_methods(&mut effective, &catalog, &invalid).is_err());
        }
        let mut missing_version = base.clone();
        missing_version.as_object_mut().unwrap().remove("version");
        let mut missing_product = base.clone();
        missing_product.as_object_mut().unwrap().remove("product");
        let mut missing_source = base.clone();
        missing_source.as_object_mut().unwrap().remove("source_id");
        let mut wrong_product = base.clone();
        wrong_product["product"] = json!("Doris");
        let mut wrong_engine = catalog.clone();
        wrong_engine["entries"][0]["engine"] = json!("mysql");
        let mut wrong_source = catalog.clone();
        wrong_source["entries"][0]["source_id"] = json!("another-source");
        for (catalog, observation) in [
            (catalog.clone(), missing_version),
            (catalog.clone(), missing_product),
            (catalog.clone(), missing_source),
            (catalog.clone(), wrong_product),
            (wrong_engine, base.clone()),
            (wrong_source, base.clone()),
        ] {
            let mut effective = json!({"datasets":[{"dataset":"s.part_day"}]});
            assert!(project_doris_capture_methods(&mut effective, &catalog, &observation).is_err());
        }
    }

    #[test]
    fn rejects_doris_identity_or_runtime_metadata_conflicts() {
        let catalog = json!({"protocol":"oo.pipeline.sealed-import-collection/v1","entries":[
            {"engine":"doris","source_id":"dorispreview","source_generation":16,"target_generation":28,"tables":[{"logical_schema":"s","logical_table":"part_day"}]}
        ]});
        let observation = json!({"protocol":"oo.source.doris.audit/v1","source_id":"dorispreview","product":"Apache Doris","version":"doris-3.1.4-rc02-7f5ba43de6c","capture_methods":{
            "s.part_day":{"logical_table":"s.part_day","capture_method":"c4_partition","consistency":"S0","delete_detection":"partition_replace","source_generation":16,"target_generation":28}
        },"table_generations":{"s.part_day":{"source_generation":16,"target_generation":28}}});
        for (key, value) in [
            ("family", json!("mysql")),
            ("product", json!("mysql")),
            ("version", json!("8.4.10")),
            ("source_kind", json!("mysql_table")),
            ("capture", json!("c5_diff")),
            ("grade", json!("B")),
            ("delete_detection", json!("reconcile")),
        ] {
            let mut source_semantics = json!({});
            source_semantics[key] = value;
            let mut effective = json!({"datasets":[{"dataset":"s.part_day","source_semantics":source_semantics}]});
            assert!(project_doris_capture_methods(&mut effective, &catalog, &observation).is_err(), "{key}");
        }
    }

    #[test]
    fn collection_contract_requires_each_real_engine_and_exact_caveats() {
        let mut catalog = json!({"protocol": SEALED_IMPORT_COLLECTION_PROTOCOL, "entries": [
            {"engine":"mariadb","source_id":"east","tables":[{"logical_schema":"cust_east","logical_table":"note","columns":[{"logical_name":"id","caveat":"bigint_unsigned_numeric"}]}]},
            {"engine":"mariadb","source_id":"west","tables":[{"logical_schema":"cust_west","logical_table":"meter","columns":[{"logical_name":"reading","caveat":"bigint_unsigned_numeric"}]}]}
        ]});
        let contract = json!({"schema":CONTRACT_SCHEMA,"family":"mariadb","product":"mariadb","version":"11.4.13","capture":"sealed_import","grade":"S0","source_kind":"mariadb","delete_detection":"reconcile","columns":[
            {"dataset":"cust_east.note","column":"id","caveat":"bigint_unsigned_numeric"},
            {"dataset":"cust_west.meter","column":"reading","caveat":"bigint_unsigned_numeric"}
        ]});
        let effective = json!({"datasets":[{"dataset":"cust_east.note"},{"dataset":"cust_west.meter"}]});
        let mut accepted = effective.clone();
        project_sealed_source_contract(&mut accepted, &catalog, Some(&contract)).unwrap();
        assert_eq!(accepted["datasets"][1]["source_semantics"]["family"], "mariadb");
        assert_eq!(catalog["entries"][0]["source_id"], "east");
        assert_eq!(catalog["entries"][1]["source_id"], "west");
        assert_eq!(project_sealed_source_contract(&mut effective.clone(), &catalog, None).unwrap_err().code, "oo.pg-builder/v1/caveat.missing_contract");
        let mut extra = contract.clone();
        extra["columns"].as_array_mut().unwrap().push(json!({"dataset":"cust_west.meter","column":"id","caveat":"bigint_unsigned_numeric"}));
        assert_eq!(project_sealed_source_contract(&mut effective.clone(), &catalog, Some(&extra)).unwrap_err().code, "oo.pg-builder/v1/caveat.rejected");
        catalog["entries"][1]["engine"] = json!("mysql");
        assert_eq!(project_sealed_source_contract(&mut effective.clone(), &catalog, Some(&contract)).unwrap_err().code, "oo.pg-builder/v1/caveat.rejected");
    }

    #[test]
    fn null_caveat_stays_unprojected_and_sidecar_is_refused() {
        let mut effective = json!({"datasets": [{"dataset": "public.codes"}]});
        let catalog = json!({
            "engine": "oracle",
            "tables": [{
                "logical_schema": "public",
                "logical_table": "codes",
                "columns": [{"logical_name": "code", "caveat": null}]
            }]
        });
        project_sealed_source_contract(&mut effective, &catalog, None).expect("null");
        assert!(effective["datasets"][0].get("source_semantics").is_none());
        let err = project_sealed_source_contract(&mut effective, &catalog, Some(&contract("enum_as_text")))
            .expect_err("sidecar");
        assert_eq!(err.code, "oo.pg-builder/v1/caveat.rejected");
    }

    #[test]
    fn unknown_and_missing_contracts_are_refused() {
        let mut effective = json!({"datasets": [{"dataset": "public.codes"}]});
        let catalog = catalog_value("time_ambiguous", 25);
        let err = project_sealed_source_contract(&mut effective, &catalog, Some(&contract("time_ambiguous")))
            .expect_err("unknown");
        assert_eq!(err.code, "oo.pg-builder/v1/caveat.rejected");
        let sealed = catalog_value("enum_as_text", 25);
        let err = project_sealed_source_contract(&mut effective, &sealed, None).expect_err("missing");
        assert_eq!(err.code, "oo.pg-builder/v1/caveat.missing_contract");
        let mut drifted = contract("enum_as_text");
        drifted["family"] = json!("mysql");
        let err = project_sealed_source_contract(&mut effective, &sealed, Some(&drifted)).expect_err("family");
        assert_eq!(err.code, "oo.pg-builder/v1/caveat.rejected");
        let split = catalog_value("precision_loss,offset_lost", 25);
        let err = project_sealed_source_contract(&mut effective, &split, Some(&contract("precision_loss")))
            .expect_err("split");
        assert_eq!(err.code, "oo.pg-builder/v1/caveat.rejected");
    }

    #[test]
    fn sealed_enum_caveat_reaches_the_role_pack_and_precision_loss_does_not() {
        let catalog = catalog_text(25, Some("enum_as_text"));
        let draft = compile_sealed_import_catalog(&catalog).expect("sealed");
        assert_eq!(
            draft.columns.iter().find(|item| item.name == "code").and_then(|item| item.caveat.as_deref()),
            Some("enum_as_text")
        );
        let settings = settings_json(&draft.identity_hash);
        let visibility = visibility_json(&draft.identity_hash);
        let decisions = json!([
            decision(&draft, "id", "identifier"),
            decision(&draft, "code", "code"),
        ]);
        let command = json!({
            "protocol": REVIEWED_SEALED_IMPORT_COMMAND_PROTOCOL,
            "catalog": serde_json::from_str::<Value>(&catalog).unwrap(),
            "settings": serde_json::from_str::<Value>(&settings).unwrap(),
            "decisions": decisions,
        });
        let reviewed = oo_builder::compile_reviewed_sealed_import(&command.to_string()).expect("reviewed");
        let reviewed_value: Value = serde_json::from_str(&reviewed).unwrap();
        let effective = reviewed_value["effective_oos"].to_string();
        let packed = bind_source_pack(&catalog, &effective, &visibility, Some(&contract("enum_as_text").to_string()))
            .expect("pack");
        let artifact = packed["artifact"].as_str().expect("artifact");
        assert!(artifact.contains("caveats: enum_as_text"), "{artifact}");
        assert!(artifact.contains("consistency_grade: S1"));
        assert!(artifact.contains("delete_detection: reconcile"));
        assert!(artifact.contains("capture: sealed_import"));
        assert!(!artifact.contains("time_ambiguous"));
        assert_eq!(packed["ready"], false);
        let loss = catalog_text(23, Some("precision_loss"));
        let err = bind_source_pack(
            &loss,
            &effective,
            &visibility,
            Some(&contract("precision_loss").to_string()),
        )
        .expect_err("precision");
        assert!(err.code.contains("type.ambiguous"), "{}", err.code);
    }

    #[test]
    fn derived_source_bound_projects_native_notes_without_a_forged_grade() {
        let artifact = pack_oracle(
            "wide",
            "catalog-wide",
            &[("id", "ID"), ("n_free_text", "N_FREE")],
        );
        assert!(artifact.contains("native: n_free_text unconstrained_number"), "{artifact}");
        assert!(artifact.contains("caveats: none"), "{artifact}");
        assert!(!artifact.contains("consistency_grade:"));
        assert!(!artifact.contains("delete_detection:"));
        assert!(!artifact.contains("caveats: unconstrained_number"));
    }

    #[test]
    fn oracle_batch_and_ts_keep_real_caveats_and_omit_grade() {
        let batch = pack_oracle(
            "batch",
            "catalog-batch",
            &[
                ("id", "ID"),
                ("n16", "N16"),
                ("n1", "N1"),
                ("d", "D"),
                ("c6", "C6"),
                ("v2", "V2"),
                ("r", "R"),
            ],
        );
        assert!(batch.contains("caveats: empty_string_is_null"), "{batch}");
        assert!(!batch.contains("consistency_grade:"));
        assert!(!batch.contains("delete_detection:"));
        assert!(!batch.contains("unconstrained_number"));
        let ts = pack_oracle(
            "ts",
            "catalog-ts",
            &[("id", "ID"), ("ts_text", "TS"), ("tstz_text", "TSTZ")],
        );
        assert!(ts.contains("caveats: precision_loss"), "{ts}");
        assert!(!ts.contains("consistency_grade:"));
        assert!(!ts.contains("delete_detection:"));
        assert!(!ts.contains("unconstrained_number"));
    }

    fn pack_oracle(kind: &str, catalog_dir: &str, bindings: &[(&str, &str)]) -> String {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../../evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/live-chain",
        );
        let catalog_text =
            std::fs::read_to_string(root.join(catalog_dir).join("catalog.json")).expect("catalog");
        let audit: Value = serde_json::from_str(
            &std::fs::read_to_string(
                root.join("identity-67")
                    .join(kind)
                    .join("audit/readonly-audit.json"),
            )
            .expect("audit"),
        )
        .expect("audit json");
        let catalog: Value = serde_json::from_str(&catalog_text).expect("catalog json");
        assert!(audit.get("consistency_grade").is_none());
        assert!(audit.get("delete_detection").is_none());
        let bound = json!({
            "protocol": source_bound::SOURCE_BOUND_PROTOCOL,
            "engine": catalog["engine"],
            "source_id": catalog["source_id"],
            "source_generation": catalog["source_generation"],
            "target_generation": catalog["target_generation"],
            "run_id": catalog["run_id"],
            "schema_digest": catalog["schema_digest"],
            "type_digest": catalog["type_digest"],
            "config_digest": catalog["config_digest"],
            "input_digest": catalog["input_digest"],
            "banner": audit["banner"],
            "scn": audit["scn"],
            "registry_snapshot": audit["registry_snapshot"],
            "published_scn": audit["published_scn"],
            "dictionary": {
                "protocol": source_bound::TYPE_DICTIONARY_PROTOCOL,
                "columns": audit["dictionary_columns"]
            },
            "bindings": bindings.iter().map(|(sealed, dictionary)| json!({
                "sealed_column": sealed,
                "dictionary_column": dictionary
            })).collect::<Vec<_>>()
        });
        let draft = compile_sealed_import_catalog(&catalog_text).expect("sealed");
        assert!(draft.columns.iter().all(|column| column.caveat.is_none()));
        assert_eq!(draft.schema_digest, catalog["schema_digest"].as_str().unwrap());
        let schema = catalog["tables"][0]["logical_schema"].as_str().expect("schema");
        let table = catalog["tables"][0]["logical_table"].as_str().expect("table");
        let dataset = format!("{schema}.{table}");
        let settings = json!({
            "project": kind,
            "ontology_version": "ont-1",
            "catalog_version": draft.identity_hash,
            "policy": {"id": format!("pol-{kind}"), "version": "1"},
            "knowledge": {"id": format!("know-{kind}"), "version": "1"},
            "ontology": {
                "spec_version": "oos/v1",
                "name": kind,
                "locale": "en-US",
                "include_schemas": [schema],
                "default_tier_preference": ["raw"],
                "publish": {"include_unreviewed": []},
                "classification": {"auto_accept": "rule_high_confidence"},
                "lifecycle": {"deprecation_min_releases": 2},
                "compile": {
                    "per_role": true,
                    "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                    "sample_values": {"default": "masked", "max_per_column": 0}
                }
            }
        });
        let names: Vec<&str> = draft.columns.iter().map(|column| column.name.as_str()).collect();
        let decisions: Vec<Value> = draft
            .columns
            .iter()
            .map(|column| {
                let semantic = if column.key { "identifier" } else { "free_text" };
                decision(&draft, &column.name, semantic)
            })
            .collect();
        let command = json!({
            "protocol": REVIEWED_SEALED_IMPORT_COMMAND_PROTOCOL,
            "catalog": catalog,
            "settings": settings,
            "decisions": decisions,
        });
        let reviewed =
            oo_builder::compile_reviewed_sealed_import(&command.to_string()).expect("reviewed");
        let reviewed_value: Value = serde_json::from_str(&reviewed).unwrap();
        let effective = reviewed_value["effective_oos"].to_string();
        let visibility = json!({
            "protocol": "oo.builder.role-visibility/v1",
            "project": kind,
            "principal": "ada",
            "role": "oo_r_clerk",
            "ontology_version": "ont-1",
            "catalog_version": draft.identity_hash,
            "policy": {"id": format!("pol-{kind}"), "version": "1"},
            "knowledge": {"id": format!("know-{kind}"), "version": "1"},
            "issued_for_compiler": "oo-builder/role-pack/v1",
            "datasets": [{
                "dataset": dataset,
                "row_security": "none",
                "global_stats": "denied",
                "columns": names
            }],
            "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
        });
        if let Ok(dir) = std::env::var("OO_U06_PACK_DUMP") {
            let dump = std::path::PathBuf::from(dir);
            let _ = std::fs::create_dir_all(&dump);
            let _ = std::fs::write(dump.join(format!("{kind}-catalog.json")), &catalog_text);
            let _ = std::fs::write(dump.join(format!("{kind}-effective.json")), &effective);
            let _ = std::fs::write(dump.join(format!("{kind}-visibility.json")), visibility.to_string());
            let _ = std::fs::write(dump.join(format!("{kind}-bound.json")), bound.to_string());
        }
        let packed = bind_source_pack(
            &catalog_text,
            &effective,
            &visibility.to_string(),
            Some(&bound.to_string()),
        )
        .expect("pack");
        assert_eq!(packed["ready"], false);
        packed["artifact"].as_str().expect("artifact").to_string()
    }

    fn decision(draft: &oo_builder::SealedImportDraft, name: &str, semantic: &str) -> Value {
        let column = draft.columns.iter().find(|item| item.name == name).expect("column");
        json!({
            "column": column.name,
            "dataset": column.dataset,
            "fact_fingerprint": column.fact_fingerprint,
            "pii": "none",
            "reviewer": {"at": "2026-10-12", "by": "ada.owner"},
            "schema_hash": draft.schema_digest,
            "semantic_type": semantic,
        })
    }

    fn contract(token: &str) -> Value {
        json!({
            "schema": CONTRACT_SCHEMA,
            "family": "oracle",
            "product": "oracle",
            "version": "19.0",
            "capture": "sealed_import",
            "grade": "S1",
            "source_kind": "oracle_table",
            "delete_detection": "reconcile",
            "columns": [{
                "dataset": "public.codes",
                "column": "code",
                "caveat": token
            }]
        })
    }

    fn catalog_value(token: &str, oid: i64) -> Value {
        serde_json::from_str(&catalog_text(oid, Some(token))).expect("catalog")
    }

    fn catalog_text(oid: i64, caveat: Option<&str>) -> String {
        let mut preimage = String::new();
        push_schema_table(&mut preimage, "public", "codes");
        push_schema_column(&mut preimage, "id", 23, -1, None);
        push_schema_column(&mut preimage, "code", oid, -1, caveat);
        let schema_digest = payload_sha256(preimage.as_bytes());
        let digest = "a".repeat(64);
        let shadow = "ab".repeat(30);
        let code_shadow = "cd".repeat(30);
        json!({
            "capture": "sealed_import",
            "config_digest": digest,
            "engine": "oracle",
            "hook_checked": false,
            "input_digest": digest,
            "protocol": "oo.pipeline.sealed-import-catalog/v1",
            "quality": {
                "loss_concealed": false,
                "manifest_reject_count": 0,
                "payload_includes_reject_lines": false,
                "stored_omits_manifest_rejects": false,
                "stored_rejected_rows": 0,
                "stored_rows": [{
                    "accepted_rows": 1,
                    "attnum": null,
                    "batch_id": "t0.b0",
                    "key_digest": null,
                    "kind": "batch",
                    "physical_name": format!("pt_{shadow}"),
                    "reason": null,
                    "rejected_rows": 0,
                    "row_index": null,
                    "value_len": null
                }]
            },
            "run_id": "beecbbc7-8dda-4779-81ba-6ff0383f72ec",
            "schema_digest": schema_digest,
            "shadows_queried": false,
            "source_generation": 1,
            "source_id": "oo04_codes",
            "source_introspection": "not_used",
            "state": "awaiting_publish",
            "tables": [{
                "accepted_rows": 1,
                "canonical_digest": digest,
                "columns": [
                    {
                        "caveat": null,
                        "key": true,
                        "logical_name": "id",
                        "oid": 23,
                        "source_type": null,
                        "stored_physical": format!("pc_{shadow}"),
                        "typmod": -1
                    },
                    {
                        "caveat": caveat,
                        "key": false,
                        "logical_name": "code",
                        "oid": oid,
                        "source_type": if oid == 25 { Value::String("text".to_string()) } else { Value::Null },
                        "stored_physical": format!("pc_{code_shadow}"),
                        "typmod": -1
                    }
                ],
                "logical_schema": "public",
                "logical_table": "codes",
                "rejects": [],
                "stored_namespace": format!("pn_{shadow}"),
                "stored_physical": format!("pt_{shadow}")
            }],
            "target_generation": 1,
            "tool_hook_digest": digest,
            "type_digest": digest
        })
        .to_string()
    }

    fn settings_json(identity: &str) -> String {
        json!({
            "project": "codes",
            "ontology_version": "ont-1",
            "catalog_version": identity,
            "policy": {"id": "pol-codes", "version": "1"},
            "knowledge": {"id": "know-codes", "version": "1"},
            "ontology": {
                "spec_version": "oos/v1",
                "name": "codes",
                "locale": "en-US",
                "include_schemas": ["public"],
                "default_tier_preference": ["raw"],
                "publish": {"include_unreviewed": []},
                "classification": {"auto_accept": "rule_high_confidence"},
                "lifecycle": {"deprecation_min_releases": 2},
                "compile": {
                    "per_role": true,
                    "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                    "sample_values": {"default": "masked", "max_per_column": 0}
                }
            }
        })
        .to_string()
    }

    fn visibility_json(identity: &str) -> String {
        json!({
            "protocol": "oo.builder.role-visibility/v1",
            "project": "codes",
            "principal": "ada",
            "role": "oo_r_clerk",
            "ontology_version": "ont-1",
            "catalog_version": identity,
            "policy": {"id": "pol-codes", "version": "1"},
            "knowledge": {"id": "know-codes", "version": "1"},
            "issued_for_compiler": "oo-builder/role-pack/v1",
            "datasets": [{
                "dataset": "public.codes",
                "row_security": "none",
                "global_stats": "denied",
                "columns": ["id", "code"]
            }],
            "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
        })
        .to_string()
    }
}

#[cfg(test)]
mod mysql_tests {
    use super::*;
    #[test]
    fn mysql_c1_projection_requires_exact_source_cut() {
        let catalog=json!({"protocol":SEALED_IMPORT_COLLECTION_PROTOCOL,"entries":[{"engine":"mysql","source_id":"mysql_owned","source_generation":4,"target_generation":4,"tables":[{"logical_schema":"db","logical_table":"t","columns":[],"rejects":[]}]}]});
        let observation=json!({"protocol":"oo.source.mysql.audit/v1","source_id":"mysql_owned","source_generation":4,"target_generation":4,"capture_method":"C1","consistency_grade":"T","delete_detection":"native","cross_table_consistent":true,"version":"8.4.10"});
        let effective=json!({"datasets":[{"dataset":"db.t"}]});
        let mut projected=effective.clone();
        project_mysql_c1(&mut projected,&catalog,&observation).unwrap();
        assert_eq!(projected["datasets"][0]["source_semantics"]["grade"],"T");
        for (key,value) in [("source_id",json!("another")),("source_generation",json!(3)),("capture_method",json!("C5")),("delete_detection",json!("none")),("cross_table_consistent",json!(false))] {
            let mut wrong=observation.clone();wrong[key]=value;
            assert!(project_mysql_c1(&mut effective.clone(),&catalog,&wrong).is_err());
        }
    }
    fn typed_fixture() -> (Value, Value, Value) {
        let catalog=json!({"protocol":SEALED_IMPORT_COLLECTION_PROTOCOL,"entries":[{"engine":"mysql","source_id":"mysql_owned","source_generation":4,"target_generation":4,"tables":[{"logical_schema":"db","logical_table":"other_names","columns":[
            {"logical_name":"d_zero","source_type":"date32","oid":1082,"caveat":null},
            {"logical_name":"d_valid","source_type":"date32","oid":1082,"caveat":null},
            {"logical_name":"elapsed","source_type":"duration_micros","oid":1186,"caveat":null}],
            "rejects":[{"column":"d_zero","attnum":1,"reason":"zero_date"}]}]}]});
        let observation=json!({"protocol":"oo.source.mysql.audit/v1","source_id":"mysql_owned","source_generation":4,"target_generation":4,"capture_method":"C1","consistency_grade":"T","delete_detection":"native","cross_table_consistent":true,"version":"5.7.44-log"});
        (json!({"datasets":[{"dataset":"db.other_names"}]}),catalog,observation)
    }

    #[test]
    fn mysql_mapping_notes_come_from_sealed_types_and_rejects_not_column_names() {
        let (mut effective,catalog,observation)=typed_fixture();
        project_mysql_c1(&mut effective,&catalog,&observation).unwrap();
        let columns=&effective["datasets"][0]["source_semantics"]["columns"];
        assert_eq!(columns.as_array().unwrap().len(),2);
        assert_eq!(columns[0]["column"],"d_zero");
        assert_eq!(columns[0]["native_note"],"mysql_zero_date_to_null_authenticated_reject");
        assert_eq!(columns[1]["column"],"elapsed");
        assert_eq!(columns[1]["native_note"],"mysql_signed_time_duration_to_interval");
    }

    #[test]
    fn mysql_datetime_zero_components_use_the_same_authenticated_null_rule() {
        let (mut effective,mut catalog,observation)=typed_fixture();
        catalog["entries"][0]["tables"][0]["columns"][0]["source_type"]=json!("timestamp_micros");
        catalog["entries"][0]["tables"][0]["columns"][0]["oid"]=json!(1114);
        project_mysql_c1(&mut effective,&catalog,&observation).unwrap();
        assert_eq!(effective["datasets"][0]["source_semantics"]["columns"][0]["native_note"],"mysql_zero_date_to_null_authenticated_reject");
    }

    #[test]
    fn mysql_mapping_notes_reject_inconsistent_sealed_types_or_reject_ordinals() {
        let (effective,catalog,observation)=typed_fixture();
        for (path,value) in [("/entries/0/tables/0/rejects/0/attnum",json!(2)),
            ("/entries/0/tables/0/columns/0/oid",json!(20)),
            ("/entries/0/tables/0/columns/2/oid",json!(1083)),
            ("/entries/0/tables/0/columns/1/caveat",json!("mysql_zero_date_to_null"))] {
            let mut wrong=catalog.clone();*wrong.pointer_mut(path).unwrap()=value;
            assert!(project_mysql_c1(&mut effective.clone(),&wrong,&observation).is_err(),"{path}");
        }
    }

    #[test]
    fn mysql_plain_nullable_dates_and_caller_notes_do_not_invent_zero_date_mapping() {
        let (mut effective,mut catalog,mut observation)=typed_fixture();
        catalog["entries"][0]["tables"][0]["rejects"]=json!([]);
        observation["native_note"]=json!("mysql_zero_date_to_null_authenticated_reject");
        observation["quality"]=json!({"zero_date_policy":"caller supplied arbitrary text"});
        project_mysql_c1(&mut effective,&catalog,&observation).unwrap();
        let columns=effective["datasets"][0]["source_semantics"]["columns"].as_array().unwrap();
        assert_eq!(columns.len(),1);
        assert_eq!(columns[0]["column"],"elapsed");
    }

}
