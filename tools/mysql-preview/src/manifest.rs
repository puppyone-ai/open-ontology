//! Tool manifest for one sealed MySQL preview import.
//!
//! The manifest keeps logical names, captured types, caveats, and masked
//! rejects. It does not carry a physical identifier or a SQL string. A
//! historical seal that omitted reject lengths stays marked that way.

use std::fs;
use std::path::Path;

use oo_pipeline::catalog_import::{push_schema_column, push_schema_table, MANIFEST_PROTOCOL};
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{json, Map, Value};

use crate::error::PreviewError;

const PREVIEW_PROTOCOL: &str = "oo.mysql-preview/v1";

/// Read the committed access and seal outcomes and write one manifest.
pub fn write_manifest(access_path: &Path, seal_path: &Path, out_path: &Path) -> Result<(), PreviewError> {
    let access = read_json(access_path)?;
    let seal = read_json(seal_path)?;
    let manifest = build_manifest(&access, &seal)?;
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|_| PreviewError::new("config", "out"))?;
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|_| PreviewError::new("config", "out"))?;
    }
    fs::write(out_path, bytes).map_err(|_| PreviewError::new("config", "out"))
}

/// Build `oo.mysql-preview.manifest/v1` from an access document and a seal.
pub fn build_manifest(access: &Value, seal: &Value) -> Result<Value, PreviewError> {
    let access_map = object(access)?;
    let seal_map = object(seal)?;
    require_text(access_map, "protocol", PREVIEW_PROTOCOL)?;
    require_text(seal_map, "protocol", PREVIEW_PROTOCOL)?;
    require_text(access_map, "state", "access")?;
    require_text(seal_map, "state", "awaiting_publish")?;
    let source_id = text(seal_map, "source_id")?;
    let config_digest = digest(seal_map, "config_digest")?;
    let schema_digest = digest(seal_map, "schema_digest")?;
    let type_digest = digest(seal_map, "type_digest")?;
    let input_digest = digest(seal_map, "input_digest")?;
    let hook_digest = digest(seal_map, "hook_digest")?;
    let source_generation = number_u64(seal_map, "source_generation")?;
    let target_generation = number_i64(seal_map, "target_generation")?;
    let run_id = text(seal_map, "run_id")?;
    if source_generation == 0 || run_id.is_empty() {
        return Err(PreviewError::new("config", "identity"));
    }
    let access_tables = array(access_map, "tables")?;
    let seal_tables = array(seal_map, "tables")?;
    if access_tables.is_empty() || access_tables.len() != seal_tables.len() || access_tables.len() > 16 {
        return Err(PreviewError::new("catalog", "coverage"));
    }
    let mut tables = Vec::new();
    let mut preimage = String::new();
    let mut saw_measured = false;
    let mut saw_unmeasured = false;
    for (access_table, seal_table) in access_tables.iter().zip(seal_tables.iter()) {
        let access_table = object(access_table)?;
        let seal_table = object(seal_table)?;
        let logical_schema = text(access_table, "schema")?;
        let logical_table = text(access_table, "table")?;
        if text(seal_table, "namespace")? != logical_schema || text(seal_table, "table")? != logical_table {
            return Err(PreviewError::new("catalog", "coverage"));
        }
        let access_columns = array(access_table, "columns")?;
        let seal_columns = array(seal_table, "columns")?;
        if access_columns.is_empty() || access_columns.len() != seal_columns.len() || access_columns.len() > 64 {
            return Err(PreviewError::new("catalog", "coverage"));
        }
        push_schema_table(&mut preimage, &logical_schema, &logical_table);
        let mut columns = Vec::new();
        let mut names = Vec::new();
        for (access_column, seal_column) in access_columns.iter().zip(seal_columns.iter()) {
            let access_column = object(access_column)?;
            let seal_column = object(seal_column)?;
            let name = text(access_column, "name")?;
            if text(seal_column, "name")? != name || name.is_empty() {
                return Err(PreviewError::new("catalog", "coverage"));
            }
            let oid = number_i64(access_column, "oid")?;
            let typmod = i32::try_from(number_i64(access_column, "typmod")?).map_err(|_| PreviewError::new("catalog", "coverage"))?;
            if number_i64(seal_column, "oid")? != oid || number_i64(seal_column, "typmod")? != i64::from(typmod) {
                return Err(PreviewError::new("catalog", "coverage"));
            }
            let caveat = match access_column.get("caveat") {
                None | Some(Value::Null) => None,
                Some(Value::String(value)) if !value.is_empty() => Some(value.clone()),
                _ => return Err(PreviewError::new("catalog", "coverage")),
            };
            let key = seal_column
                .get("key")
                .and_then(Value::as_bool)
                .ok_or_else(|| PreviewError::new("catalog", "coverage"))?;
            push_schema_column(&mut preimage, &name, oid, typmod, caveat.as_deref());
            names.push(name.clone());
            columns.push(json!({
                "caveat": caveat,
                "key": key,
                "logical_name": name,
                "oid": oid,
                "typmod": typmod
            }));
        }
        let accepted_rows = number_u64(seal_table, "rows")?;
        let canonical_digest = digest(seal_table, "canonical_digest")?;
        let mut rejects = Vec::new();
        for reject in array(seal_table, "rejects")? {
            let reject = object(reject)?;
            if text(reject, "table")? != logical_table {
                return Err(PreviewError::new("catalog", "coverage"));
            }
            let column = text(reject, "column")?;
            if !names.iter().any(|name| name == &column) {
                return Err(PreviewError::new("catalog", "coverage"));
            }
            let reason = text(reject, "reason")?;
            if !closed_reason(&reason) {
                return Err(PreviewError::new("value", "quality"));
            }
            let row_index = number_u64(reject, "row")?;
            let attnum = optional_i64(reject, "attnum")?;
            let value_len = optional_i64(reject, "value_len")?;
            match (attnum, value_len) {
                (Some(_), Some(_)) => saw_measured = true,
                (None, None) => saw_unmeasured = true,
                _ => return Err(PreviewError::new("value", "quality")),
            }
            let mut encoded = Map::new();
            if let Some(attnum) = attnum {
                let attnum = i32::try_from(attnum).map_err(|_| PreviewError::new("value", "quality"))?;
                encoded.insert("attnum".to_string(), json!(attnum));
            }
            encoded.insert("column".to_string(), json!(column));
            if let Some(key_digest) = optional_text(reject, "key_digest")?
                && !key_digest.is_empty()
            {
                encoded.insert("key_digest".to_string(), json!(key_digest));
            }
            encoded.insert("reason".to_string(), json!(reason));
            encoded.insert("row_index".to_string(), json!(row_index));
            if let Some(value_len) = value_len {
                encoded.insert("value_len".to_string(), json!(value_len));
            }
            rejects.push(Value::Object(encoded));
        }
        tables.push(json!({
            "accepted_rows": accepted_rows,
            "canonical_digest": canonical_digest,
            "columns": columns,
            "logical_schema": logical_schema,
            "logical_table": logical_table,
            "rejects": rejects
        }));
    }
    if saw_measured && saw_unmeasured {
        return Err(PreviewError::new("value", "quality"));
    }
    if sha256_hex(preimage.as_bytes()) != schema_digest {
        return Err(PreviewError::new("catalog", "schema_digest"));
    }
    let manifest = json!({
        "config_digest": config_digest,
        "engine": "mysql",
        "hook_digest": hook_digest,
        "input_digest": input_digest,
        "payload_includes_reject_lines": saw_measured,
        "protocol": MANIFEST_PROTOCOL,
        "run_id": run_id,
        "schema_digest": schema_digest,
        "source_generation": source_generation,
        "source_id": source_id,
        "tables": tables,
        "target_generation": target_generation,
        "type_digest": type_digest
    });
    let encoded = serde_json::to_string(&manifest).map_err(|_| PreviewError::new("config", "out"))?;
    if encoded.contains("physical") || encoded.contains("\"sql\"") || encoded.contains("oo.source.pgsql/v1") {
        return Err(PreviewError::new("catalog", "manifest"));
    }
    Ok(manifest)
}

fn read_json(path: &Path) -> Result<Value, PreviewError> {
    let bytes = fs::read(path).map_err(|_| PreviewError::new("config", "path"))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(PreviewError::new("budget", "max_bytes"));
    }
    serde_json::from_slice(&bytes).map_err(|_| PreviewError::new("config", "json"))
}

fn object(value: &Value) -> Result<&Map<String, Value>, PreviewError> {
    value.as_object().ok_or_else(|| PreviewError::new("config", "json"))
}

fn array<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a Vec<Value>, PreviewError> {
    map.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| PreviewError::new("config", "json"))
}

fn text(map: &Map<String, Value>, key: &str) -> Result<String, PreviewError> {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| PreviewError::new("config", "json"))
}

fn require_text(map: &Map<String, Value>, key: &str, expected: &str) -> Result<(), PreviewError> {
    if map.get(key).and_then(Value::as_str) == Some(expected) {
        Ok(())
    } else {
        Err(PreviewError::new("protocol", "protocol"))
    }
}

fn digest(map: &Map<String, Value>, key: &str) -> Result<String, PreviewError> {
    let value = text(map, key)?;
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        Ok(value)
    } else {
        Err(PreviewError::new("config", "identity"))
    }
}

fn number_i64(map: &Map<String, Value>, key: &str) -> Result<i64, PreviewError> {
    map.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| PreviewError::new("config", "json"))
}

fn number_u64(map: &Map<String, Value>, key: &str) -> Result<u64, PreviewError> {
    map.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| PreviewError::new("config", "json"))
}

fn optional_i64(map: &Map<String, Value>, key: &str) -> Result<Option<i64>, PreviewError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_i64().map(Some).ok_or_else(|| PreviewError::new("value", "quality")),
    }
}

fn optional_text(map: &Map<String, Value>, key: &str) -> Result<Option<String>, PreviewError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(PreviewError::new("value", "quality")),
    }
}

fn closed_reason(value: &str) -> bool {
    (1..=80).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_of(columns: &[(&str, i64, i32, Option<&str>)]) -> String {
        let mut preimage = String::new();
        push_schema_table(&mut preimage, "erp", "parcel");
        for (name, oid, typmod, caveat) in columns {
            push_schema_column(&mut preimage, name, *oid, *typmod, *caveat);
        }
        sha256_hex(preimage.as_bytes())
    }

    #[test]
    fn historical_manifest_omits_physical_names_and_unmeasured_lengths() {
        let columns = [("id", 23, -1, None), ("day_on", 1082, -1, None)];
        let schema_digest = digest_of(&columns);
        let access = json!({
            "protocol": "oo.mysql-preview/v1",
            "state": "access",
            "tables": [{
                "schema": "erp",
                "table": "parcel",
                "columns": [
                    {"name": "id", "oid": 23, "typmod": -1, "caveat": null},
                    {"name": "day_on", "oid": 1082, "typmod": -1, "caveat": null}
                ]
            }]
        });
        let seal = json!({
            "protocol": "oo.mysql-preview/v1",
            "state": "awaiting_publish",
            "source_id": "mysqlpreview",
            "config_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "schema_digest": schema_digest,
            "type_digest": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "input_digest": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "hook_digest": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            "source_generation": 1,
            "target_generation": 3,
            "run_id": "de8276fc-8634-4543-994f-ce82a46a3c18",
            "tables": [{
                "namespace": "erp",
                "table": "parcel",
                "physical": "pt_9cb24f0ac8a48d5f4f4f6d93f5ebe7f2c4162c9ae5237040365d3f2047ce",
                "rows": 5,
                "canonical_digest": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                "columns": [
                    {"name": "id", "oid": 23, "typmod": -1, "key": true, "physical": "pc_aa"},
                    {"name": "day_on", "oid": 1082, "typmod": -1, "key": false, "physical": "pc_bb"}
                ],
                "rejects": [{"table": "parcel", "column": "day_on", "row": 1, "reason": "zero_date"}]
            }]
        });
        let manifest = build_manifest(&access, &seal).expect("manifest");
        let encoded = serde_json::to_string(&manifest).expect("json");
        assert!(!encoded.contains("physical"));
        assert!(!encoded.contains("pt_"));
        assert!(!encoded.contains("pc_"));
        assert!(!encoded.contains("attnum"));
        assert!(!encoded.contains("value_len"));
        assert_eq!(manifest["engine"], "mysql");
        assert_eq!(manifest["payload_includes_reject_lines"], false);
        assert_eq!(manifest["tables"][0]["rejects"][0]["row_index"], 1);
        assert_eq!(manifest["tables"][0]["rejects"][0]["reason"], "zero_date");
    }

    #[test]
    fn measured_rejects_keep_length_and_attnum() {
        let columns = [("id", 23, -1, None)];
        let schema_digest = digest_of(&columns);
        let access = json!({
            "protocol": "oo.mysql-preview/v1",
            "state": "access",
            "tables": [{
                "schema": "erp",
                "table": "parcel",
                "columns": [{"name": "id", "oid": 23, "typmod": -1, "caveat": null}]
            }]
        });
        let seal = json!({
            "protocol": "oo.mysql-preview/v1",
            "state": "awaiting_publish",
            "source_id": "mysqlpreview",
            "config_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "schema_digest": schema_digest,
            "type_digest": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "input_digest": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "hook_digest": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            "source_generation": 1,
            "target_generation": 3,
            "run_id": "de8276fc-8634-4543-994f-ce82a46a3c18",
            "tables": [{
                "namespace": "erp",
                "table": "parcel",
                "rows": 1,
                "canonical_digest": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                "columns": [{"name": "id", "oid": 23, "typmod": -1, "key": true}],
                "rejects": [{
                    "table": "parcel",
                    "column": "id",
                    "row": 0,
                    "reason": "zero_date",
                    "attnum": 1,
                    "value_len": 10
                }]
            }]
        });
        let manifest = build_manifest(&access, &seal).expect("manifest");
        assert_eq!(manifest["payload_includes_reject_lines"], true);
        assert_eq!(manifest["tables"][0]["rejects"][0]["attnum"], 1);
        assert_eq!(manifest["tables"][0]["rejects"][0]["value_len"], 10);
    }

    #[test]
    fn schema_digest_must_match_the_access_preimage() {
        let access = json!({
            "protocol": "oo.mysql-preview/v1",
            "state": "access",
            "tables": [{
                "schema": "erp",
                "table": "parcel",
                "columns": [{"name": "id", "oid": 23, "typmod": -1, "caveat": null}]
            }]
        });
        let seal = json!({
            "protocol": "oo.mysql-preview/v1",
            "state": "awaiting_publish",
            "source_id": "mysqlpreview",
            "config_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "schema_digest": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "type_digest": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "input_digest": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "hook_digest": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            "source_generation": 1,
            "target_generation": 3,
            "run_id": "de8276fc-8634-4543-994f-ce82a46a3c18",
            "tables": [{
                "namespace": "erp",
                "table": "parcel",
                "rows": 0,
                "canonical_digest": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                "columns": [{"name": "id", "oid": 23, "typmod": -1, "key": true}],
                "rejects": []
            }]
        });
        assert!(matches!(
            build_manifest(&access, &seal),
            Err(PreviewError { code: "catalog", fact: "schema_digest" })
        ));
    }

    #[test]
    fn captured_column_preimage_matches_the_sealed_schema_digest() {
        let parcel = [
            ("id", 23_i64, -1_i32, None),
            ("qty", 21, -1, None),
            ("flag", 21, -1, Some("tinyint1_as_int2")),
            ("units", 20, -1, None),
            ("big_u", 1700, 1310724, Some("bigint_unsigned_numeric")),
            ("amount", 1700, 786438, None),
            ("label", 25, -1, None),
            ("note", 1043, 44, None),
            ("blob_col", 17, -1, None),
            ("when_at", 1114, -1, None),
            ("when_utc", 1184, -1, None),
            ("day_on", 1082, -1, None),
            ("clock_at", 1083, -1, Some("mysql_time_not_interval")),
            ("over_time", 1083, -1, Some("mysql_time_not_interval")),
            ("payload", 114, -1, Some("json_not_jsonb")),
            ("lane", 25, -1, Some("enum_as_text")),
            ("rate", 700, -1, None),
            ("score", 701, -1, None),
        ];
        let mut preimage = String::new();
        push_schema_table(&mut preimage, "erp", "parcel");
        for (name, oid, typmod, caveat) in parcel {
            push_schema_column(&mut preimage, name, oid, typmod, caveat);
        }
        push_schema_table(&mut preimage, "erp", "empty_box");
        push_schema_column(&mut preimage, "id", 23, -1, None);
        assert_eq!(
            sha256_hex(preimage.as_bytes()),
            "bac1dde9dc73fc98cf5531621818af2a5d64832a25b6dd60fb52d76942f5cea8"
        );
    }
}
