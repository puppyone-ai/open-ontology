//! Capture receipt for one real OpenSearch cycle.
//!
//! The receipt bytes and the native load document are written from the same
//! inputs: the Arrow stream, the reader audit, the customer config, the cycle
//! catalog, the registry file, the capture driver, and the cycle snapshot.
//! Digests are hashes of those files. The receipt does not copy `secret_ref`.

use std::io::Cursor;

use arrow_array::cast::AsArray;
use arrow_array::Array;
use arrow_ipc::reader::StreamReader;
use arrow_schema::{DataType, TimeUnit};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub const RECEIPT_SCHEMA: &str = "oo.source-admission.capture-receipt/v1";

pub struct CycleFiles {
    pub arrow: Vec<u8>,
    pub audit: Vec<u8>,
    pub config: Vec<u8>,
    pub catalog: Vec<u8>,
    pub registry: Vec<u8>,
    pub driver: Vec<u8>,
    pub snapshot: Vec<u8>,
    pub prior_snapshot: Option<Vec<u8>>,
    pub result: Vec<u8>,
}

pub struct LoadPaths {
    pub arrow_path: String,
    pub admission_path: String,
    pub password_file: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub runtime_principal: String,
}

pub struct Produced {
    pub receipt: Value,
    pub request: Value,
    pub receipt_sha256: String,
    pub tombstones: Vec<String>,
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn produce(
    files: &CycleFiles,
    paths: &LoadPaths,
    generation: i64,
    parent: Option<i64>,
    capture_watermark: i64,
    batch_id: &str,
    logical_schema: &str,
    logical_table: &str,
    key_column: &str,
) -> Result<Produced, String> {
    if generation < 1 {
        return Err("generation".into());
    }
    if parent.is_some_and(|value| value.checked_add(1) != Some(generation)) || (parent.is_none() && generation != 1) {
        return Err("parent_generation".into());
    }
    if !batch_token(batch_id) || !batch_token(logical_schema) || !batch_token(logical_table) {
        return Err("token".into());
    }
    let config: Value = serde_json::from_slice(&files.config).map_err(|error| format!("config:{error}"))?;
    let source_id = config
        .get("source_id")
        .and_then(Value::as_str)
        .filter(|text| ident_text(text))
        .ok_or_else(|| "config_source_id".to_string())?;
    let columns = declare_columns(&files.arrow)?;
    if !columns.iter().any(|column| {
        column["name"] == key_column && column["arrow_type"] == "utf8" && column["nullable"] == false
    }) {
        return Err(format!("key_column:{key_column}"));
    }
    let result: Value = serde_json::from_slice(&files.result).map_err(|error| format!("result:{error}"))?;
    let snapshot: Value = serde_json::from_slice(&files.snapshot).map_err(|error| format!("snapshot:{error}"))?;
    let prior = match &files.prior_snapshot {
        None => None,
        Some(bytes) => Some(serde_json::from_slice(bytes).map_err(|error| format!("prior_snapshot:{error}"))?),
    };
    let tombstones = tombstone_ids(&result, &snapshot, prior.as_ref())?;
    let live_keys = utf8_keys(&files.arrow, key_column)?;
    if tombstones.iter().any(|id| live_keys.iter().any(|live| live == id)) {
        return Err("tombstone_overlaps_live_key".into());
    }
    let arrow_sha = sha256_hex(&files.arrow);
    let arrow_schema = schema_message_sha256(&files.arrow)?;
    let column_bytes = serde_json::to_vec(&columns).map_err(|error| format!("columns:{error}"))?;
    let run_id = format!("os44-g{generation}-{}", &arrow_sha[..16]);
    let snapshot_value = sha256_hex(&files.snapshot);
    let mut receipt = Map::new();
    receipt.insert("schema".into(), json!(RECEIPT_SCHEMA));
    receipt.insert("source_engine".into(), json!("opensearch"));
    receipt.insert("source_id".into(), json!(source_id));
    receipt.insert("source_generation".into(), json!(generation));
    receipt.insert("capture_watermark".into(), json!(capture_watermark));
    receipt.insert("capture_method".into(), json!("C5"));
    receipt.insert("consistency".into(), json!("B"));
    receipt.insert("delete_detection".into(), json!("reconcile"));
    receipt.insert("snapshot".into(), json!({"kind": "c5_cycle", "value": snapshot_value}));
    receipt.insert("run_id".into(), json!(run_id));
    receipt.insert("schema_digest".into(), json!(sha256_hex(&files.catalog)));
    receipt.insert("type_digest".into(), json!(sha256_hex(&column_bytes)));
    receipt.insert("config_digest".into(), json!(sha256_hex(&files.config)));
    receipt.insert("input_digest".into(), json!(arrow_sha));
    receipt.insert("audit_digest".into(), json!(sha256_hex(&files.audit)));
    receipt.insert("catalog_sha256".into(), json!(sha256_hex(&files.catalog)));
    receipt.insert("registry_sha256".into(), json!(sha256_hex(&files.registry)));
    receipt.insert("driver_digest".into(), json!(sha256_hex(&files.driver)));
    receipt.insert("arrow_sha256".into(), json!([arrow_sha]));
    receipt.insert("arrow_schema_sha256".into(), json!(arrow_schema));
    receipt.insert("batch_id".into(), json!(batch_id));
    receipt.insert("logical_schema".into(), json!(logical_schema));
    receipt.insert("logical_table".into(), json!(logical_table));
    receipt.insert("key_column".into(), json!(key_column));
    receipt.insert("tombstones".into(), json!(tombstones));
    let receipt = Value::Object(receipt);
    reject_secret(&receipt)?;
    let receipt_bytes = serde_json::to_vec(&receipt).map_err(|error| format!("receipt:{error}"))?;
    let receipt_sha256 = sha256_hex(&receipt_bytes);
    let mut request = Map::new();
    request.insert("protocol".into(), json!("oo.pipeline.native-update/v1"));
    request.insert("source_engine".into(), receipt["source_engine"].clone());
    request.insert("source_id".into(), receipt["source_id"].clone());
    request.insert("source_generation".into(), json!(generation));
    request.insert(
        "parent_source_generation".into(),
        parent.map_or(Value::Null, |value| json!(value)),
    );
    request.insert("logical_schema".into(), json!(logical_schema));
    request.insert("logical_table".into(), json!(logical_table));
    request.insert("batch_id".into(), json!(batch_id));
    request.insert("capture_method".into(), json!("c5_diff"));
    request.insert("delete_detection".into(), json!("reconcile"));
    request.insert("consistency_grade".into(), json!("B"));
    request.insert("cross_table_consistent".into(), json!(false));
    request.insert("position_codec".into(), json!("snapshot-zero/v1"));
    request.insert("position_lineage".into(), json!("opensearch.c5.cycle"));
    request.insert("applied_pos".into(), json!("0"));
    for name in ["config_digest", "schema_digest", "type_digest", "input_digest", "audit_digest"] {
        request.insert(name.into(), receipt[name].clone());
    }
    request.insert("snapshot".into(), receipt["snapshot"].clone());
    request.insert("columns".into(), Value::Array(columns));
    request.insert("arrow_path".into(), json!(paths.arrow_path));
    request.insert("tombstones".into(), json!(tombstones));
    request.insert("row_identity".into(), json!("unique"));
    request.insert("key_column".into(), json!(key_column));
    request.insert("mirror".into(), json!(false));
    request.insert(
        "target".into(),
        json!({
            "host": paths.host,
            "port": paths.port,
            "database": paths.database,
            "runtime_principal": paths.runtime_principal,
            "password_file": paths.password_file,
            "ssl": "disable",
            "shard": format!("shard_{source_id}"),
            "owner": owner_uuid(source_id),
        }),
    );
    request.insert("admission_path".into(), json!(paths.admission_path));
    request.insert(
        "admission_expect".into(),
        json!({"declared_receipt_sha256": receipt_sha256}),
    );
    let request = Value::Object(request);
    reject_secret(&request)?;
    Ok(Produced {
        receipt,
        request,
        receipt_sha256,
        tombstones,
    })
}

pub fn schema_message_sha256(bytes: &[u8]) -> Result<String, String> {
    let interior;
    let source: &[u8] = if bytes.starts_with(b"ARROW1") {
        interior = file_interior(bytes)?;
        &interior
    } else {
        bytes
    };
    let frames = frame_messages(source)?;
    for meta in frames {
        if meta.is_empty() {
            continue;
        }
        if message_header_type(meta)? == Some(1) {
            return Ok(sha256_hex(meta));
        }
    }
    Err("arrow_schema_message_missing".into())
}

fn declare_columns(arrow: &[u8]) -> Result<Vec<Value>, String> {
    let reader = StreamReader::try_new(Cursor::new(arrow), None).map_err(|error| format!("arrow:{error}"))?;
    let schema = reader.schema();
    if schema.fields().is_empty() {
        return Err("arrow_schema_empty".into());
    }
    let mut columns = Vec::with_capacity(schema.fields().len());
    for field in schema.fields() {
        let (arrow_type, precision) = loader_type(field.data_type())?;
        let mut column = Map::new();
        column.insert("name".into(), json!(field.name()));
        column.insert("logical_name".into(), json!(field.name()));
        column.insert("arrow_type".into(), json!(arrow_type));
        column.insert("nullable".into(), json!(field.is_nullable()));
        if let Some(precision) = precision {
            column.insert("precision".into(), json!(precision));
        }
        columns.push(Value::Object(column));
    }
    Ok(columns)
}

fn loader_type(dtype: &DataType) -> Result<(&'static str, Option<i32>), String> {
    let mapped = match dtype {
        DataType::Utf8 => ("utf8", None),
        DataType::Binary => ("binary", None),
        DataType::Boolean => ("bool", None),
        DataType::Int64 => ("int64", Some(64)),
        DataType::Float64 => ("float64", None),
        DataType::Timestamp(TimeUnit::Microsecond, Some(_)) => ("timestamp_micros_utc", None),
        DataType::List(item) if item.data_type() == &DataType::Binary => ("list_binary", None),
        DataType::List(item) if item.data_type() == &DataType::Int64 => ("list_int64", None),
        DataType::List(item) if item.data_type() == &DataType::Utf8 => ("list(utf8)", None),
        other => return Err(format!("arrow_type_unsupported:{other}")),
    };
    Ok(mapped)
}

fn utf8_keys(arrow: &[u8], key_column: &str) -> Result<Vec<String>, String> {
    let reader = StreamReader::try_new(Cursor::new(arrow), None).map_err(|error| format!("arrow:{error}"))?;
    let mut keys = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|error| format!("arrow_batch:{error}"))?;
        let column = batch
            .column_by_name(key_column)
            .ok_or_else(|| format!("key_column:{key_column}"))?;
        let values = column.as_string_opt::<i32>().ok_or_else(|| format!("key_type:{key_column}"))?;
        for index in 0..values.len() {
            let text = values.value(index);
            if text.is_empty() {
                return Err("empty_key".into());
            }
            keys.push(text.to_string());
        }
    }
    Ok(keys)
}

fn tombstone_ids(result: &Value, snapshot: &Value, prior: Option<&Value>) -> Result<Vec<String>, String> {
    let names = result
        .pointer("/report/tombstoned")
        .and_then(Value::as_array)
        .ok_or_else(|| "result_tombstoned".to_string())?;
    let mut index = doc_index(snapshot)?;
    if let Some(prior) = prior {
        for (key, id) in doc_index(prior)? {
            index.entry(key).or_insert(id);
        }
    }
    let mut ids = Vec::with_capacity(names.len());
    for name in names {
        let key = name.as_str().ok_or_else(|| "tombstone_key".to_string())?;
        let id = index.get(key).ok_or_else(|| format!("tombstone_unmapped:{key}"))?;
        ids.push(id.clone());
    }
    Ok(ids)
}

fn doc_index(snapshot: &Value) -> Result<std::collections::BTreeMap<String, String>, String> {
    let docs = snapshot
        .get("docs")
        .and_then(Value::as_array)
        .ok_or_else(|| "snapshot_docs".to_string())?;
    let mut index = std::collections::BTreeMap::new();
    for doc in docs {
        let id = doc.get("id").and_then(Value::as_str).ok_or_else(|| "snapshot_id".to_string())?;
        let raw = doc.get("raw").and_then(Value::as_str).ok_or_else(|| "snapshot_raw".to_string())?;
        let parsed: Value = serde_json::from_str(raw).map_err(|error| format!("snapshot_raw:{error}"))?;
        let key = parsed
            .get("doc_key")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("snapshot_doc_key:{id}"))?;
        index.insert(key.to_string(), id.to_string());
    }
    Ok(index)
}

fn reject_secret(value: &Value) -> Result<(), String> {
    let text = serde_json::to_string(value).map_err(|error| format!("json:{error}"))?;
    if text.contains("secret://") || text.contains("\"secret_ref\"") {
        return Err("secret_in_binding".into());
    }
    Ok(())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn owner_uuid(source_id: &str) -> String {
    let hex = sha256_hex(source_id.as_bytes());
    format!(
        "{}-{}-4{}-a{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    )
}

fn ident_text(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn batch_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

fn file_interior(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < 16 || &bytes[bytes.len() - 6..] != b"ARROW1" {
        return Err("arrow_file".into());
    }
    let size_at = bytes.len() - 10;
    let footer_len = i32::from_le_bytes(bytes[size_at..size_at + 4].try_into().unwrap());
    if footer_len <= 0 {
        return Err("arrow_file".into());
    }
    let footer_len = usize::try_from(footer_len).map_err(|_| "arrow_file")?;
    let footer_start = size_at.checked_sub(footer_len).ok_or("arrow_file")?;
    let mut at = 6usize;
    if !at.is_multiple_of(8) {
        at = align8(at);
    }
    while at + 8 <= footer_start {
        let word = i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        if word == -1 {
            break;
        }
        at += 8;
    }
    Ok(bytes[at..footer_start].to_vec())
}

fn frame_messages(bytes: &[u8]) -> Result<Vec<&[u8]>, String> {
    let mut offset = 0usize;
    let mut out = Vec::new();
    while offset + 8 <= bytes.len() {
        let continuation = i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let meta_len = i32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
        if continuation != -1 || meta_len < 0 {
            return Err("arrow_frame".into());
        }
        if meta_len == 0 {
            break;
        }
        let meta_bytes = usize::try_from(meta_len).map_err(|_| "arrow_frame")?;
        let meta_end = offset + 8 + meta_bytes;
        if meta_end > bytes.len() {
            return Err("arrow_frame".into());
        }
        let meta = &bytes[offset + 8..meta_end];
        out.push(meta);
        let body_len = message_body_len(meta)?;
        let body_end = align8(meta_end).checked_add(body_len).ok_or("arrow_frame")?;
        if body_end > bytes.len() {
            return Err("arrow_frame".into());
        }
        offset = if body_end.is_multiple_of(8) { body_end } else { align8(body_end) };
    }
    Ok(out)
}

fn message_header_type(meta: &[u8]) -> Result<Option<u8>, String> {
    let root = root_start(meta)?;
    field_u8(meta, root, 1)
}

fn message_body_len(meta: &[u8]) -> Result<usize, String> {
    let root = root_start(meta)?;
    let Some(at) = field_offset(meta, root, 3)? else {
        return Ok(0);
    };
    if at + 8 > meta.len() {
        return Err("arrow_frame".into());
    }
    let len = i64::from_le_bytes(meta[at..at + 8].try_into().unwrap());
    usize::try_from(len).map_err(|_| "arrow_frame".into())
}

fn root_start(meta: &[u8]) -> Result<usize, String> {
    if meta.len() < 4 {
        return Err("arrow_frame".into());
    }
    Ok(u32::from_le_bytes(meta[0..4].try_into().unwrap()) as usize)
}

fn field_u8(meta: &[u8], start: usize, index: usize) -> Result<Option<u8>, String> {
    let Some(at) = field_offset(meta, start, index)? else {
        return Ok(None);
    };
    meta.get(at).copied().ok_or_else(|| "arrow_frame".into()).map(Some)
}

fn field_offset(meta: &[u8], start: usize, index: usize) -> Result<Option<usize>, String> {
    if start + 4 > meta.len() {
        return Err("arrow_frame".into());
    }
    let soff = i32::from_le_bytes(meta[start..start + 4].try_into().unwrap());
    let start_i = isize::try_from(start).map_err(|_| "arrow_frame")?;
    let soff_i = isize::try_from(soff).map_err(|_| "arrow_frame")?;
    let vtable = usize::try_from(start_i.checked_sub(soff_i).ok_or("arrow_frame")?).map_err(|_| "arrow_frame")?;
    if vtable + 2 > meta.len() {
        return Err("arrow_frame".into());
    }
    let vsize = u16::from_le_bytes(meta[vtable..vtable + 2].try_into().unwrap()) as usize;
    let slot = 4 + index * 2;
    if slot + 2 > vsize {
        return Ok(None);
    }
    let rel = u16::from_le_bytes(meta[vtable + slot..vtable + slot + 2].try_into().unwrap()) as usize;
    if rel == 0 {
        Ok(None)
    } else {
        Ok(Some(start + rel))
    }
}

fn align8(value: usize) -> usize {
    value.saturating_add(7) & !7
}

#[cfg(test)]
mod tests {
    use super::{produce, schema_message_sha256, CycleFiles, LoadPaths};
    use crate::cast::FieldPlan;
    use crate::catalog::EncodedRow;
    use crate::ipc::write_ipc;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn plan(name: &str, os_type: &str) -> FieldPlan {
        FieldPlan {
            name: name.into(),
            os_type: os_type.into(),
            nullable: true,
            unsupported: false,
            unsupported_reason: None,
            caveat: None,
            scaling_factor: None,
            children: Vec::new(),
            json_text: os_type == "object",
            metadata: BTreeMap::new(),
        }
    }

    fn row(id: &str, doc_key: &str) -> EncodedRow {
        let mut values = BTreeMap::new();
        values.insert("doc_key".into(), (true, crate::cast::Cell::Text(doc_key.into())));
        EncodedRow {
            source_doc_id: id.into(),
            source_index: "oo-dl01w1-44-ledger".into(),
            raw_source: format!(r#"{{"doc_key":"{doc_key}"}}"#).into_bytes(),
            values,
        }
    }

    #[test]
    fn receipt_uses_file_hashes_and_omits_the_secret() {
        let arrow = write_ipc(&[plan("doc_key", "keyword")], &[row("led-01", "ledger-open")], BTreeMap::new()).unwrap();
        let snapshot = json!({"docs":[{"id":"led-01","raw":"{\"doc_key\":\"ledger-open\"}"}]});
        let result = json!({"report":{"tombstoned":[]}});
        let config = br#"{"source_id":"oo_dl01w1_44","secret_ref":"secret://oo-dl01w1-44/opensearch"}"#;
        let produced = produce(
            &CycleFiles {
                arrow: arrow.clone(),
                audit: b"audit-line\n".to_vec(),
                config: config.to_vec(),
                catalog: b"{\"fields\":1}".to_vec(),
                registry: b"{\"source_id\":\"oo_dl01w1_44\"}".to_vec(),
                driver: b"driver-bytes".to_vec(),
                snapshot: serde_json::to_vec(&snapshot).unwrap(),
                prior_snapshot: None,
                result: serde_json::to_vec(&result).unwrap(),
            },
            &LoadPaths {
                arrow_path: "/in/batch.arrow".into(),
                admission_path: "/in/receipt.json".into(),
                password_file: "/in/pw".into(),
                host: "127.0.0.1".into(),
                port: 5432,
                database: "oo_dl01w1_44".into(),
                runtime_principal: "oo_dl01w1_44_runtime".into(),
            },
            1,
            None,
            1,
            "os44-fast-1",
            "ledger",
            "ledger_rows",
            "source_doc_id",
        )
        .unwrap();
        let text = serde_json::to_string(&produced.receipt).unwrap();
        assert!(!text.contains("secret"));
        assert_eq!(produced.receipt["arrow_sha256"][0], super::sha256_hex(&arrow));
        assert_eq!(produced.receipt["arrow_schema_sha256"], schema_message_sha256(&arrow).unwrap());
        assert_eq!(produced.request["admission_expect"]["declared_receipt_sha256"], produced.receipt_sha256);
        assert_eq!(produced.request["source_generation"], 1);
        assert!(produced.request["parent_source_generation"].is_null());
        assert_eq!(produced.request["columns"][0]["name"], "source_doc_id");
        assert_eq!(produced.request["columns"][0]["nullable"], false);
    }

    #[test]
    fn deleted_doc_key_becomes_the_source_id_and_cannot_overlap() {
        let arrow = write_ipc(&[plan("doc_key", "keyword")], &[row("led-01", "ledger-open")], BTreeMap::new()).unwrap();
        let snapshot = json!({"docs":[{"id":"led-01","raw":"{\"doc_key\":\"ledger-open\"}"}]});
        let prior = json!({"docs":[{"id":"led-c5","raw":"{\"doc_key\":\"ledger-c5\"}"}]});
        let result = json!({"report":{"tombstoned":["ledger-c5"]}});
        let produced = produce(
            &CycleFiles {
                arrow,
                audit: b"a".to_vec(),
                config: br#"{"source_id":"oo_dl01w1_44"}"#.to_vec(),
                catalog: b"c".to_vec(),
                registry: b"r".to_vec(),
                driver: b"d".to_vec(),
                snapshot: serde_json::to_vec(&snapshot).unwrap(),
                prior_snapshot: Some(serde_json::to_vec(&prior).unwrap()),
                result: serde_json::to_vec(&result).unwrap(),
            },
            &LoadPaths {
                arrow_path: "/a".into(),
                admission_path: "/r".into(),
                password_file: "/p".into(),
                host: "127.0.0.1".into(),
                port: 5432,
                database: "oo_dl01w1_44".into(),
                runtime_principal: "oo_dl01w1_44_runtime".into(),
            },
            2,
            Some(1),
            8,
            "os44-fast-8",
            "ledger",
            "ledger_rows",
            "source_doc_id",
        )
        .unwrap();
        assert_eq!(produced.tombstones, vec!["led-c5".to_string()]);
        assert_eq!(produced.receipt["capture_watermark"], 8);
        assert_eq!(produced.request["source_generation"], 2);
        assert_eq!(produced.request["parent_source_generation"], 1);
    }
}
