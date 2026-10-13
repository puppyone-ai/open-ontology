//! Full-document seal for the approved shipment rows.
//!
//! Raw BSON is required. `weight` stays in the document as Canonical Extended
//! JSON `$numberDouble`. A missing approved key stays absent. An unapproved
//! root key, including `hiddenNote`, is `field.scope` and is not removed.
//! This module does not open a database and does not start an import.

use serde_json::{Value, json};

use oo_builder::compile_jsonb_document_profile;
use oo_core::api::payload_sha256;
use oo_core::jsonb_doc::{self, DOUBLE_ENCODING};
use oo_core::sealed_import::{self, push_schema_column, push_schema_table};

pub const PROTOCOL: &str = "oo.mongodb-preview.docseal/v1";
pub const IMPORTER_PROTOCOL: &str = "oo.pipeline.jsonb-document-import/v1";

pub const APPROVED_FIELDS: &[&str] = &[
    "_id",
    "code",
    "flag",
    "units",
    "price",
    "created",
    "uid",
    "blob",
    "blob_presence",
    "weight",
    "warehouse",
    "whCode",
    "lane",
    "memo",
    "memo_presence",
    "marks",
    "marks_presence",
    "pack",
    "pack_presence",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealFault {
    pub code: String,
    pub path: String,
    pub message: &'static str,
}

struct Element {
    kind: u8,
    key: String,
    payload: Vec<u8>,
}

pub fn decode_hex(text: &str) -> Result<Vec<u8>, SealFault> {
    let bytes = text.trim().as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) || bytes.len() > 2_097_152 {
        return Err(fault(
            "schema.invalid",
            "/raw_bson_hex",
            "raw BSON hex is not an even byte string",
        ));
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index < bytes.len() {
        let high = hex_nibble(bytes[index])?;
        let low = hex_nibble(bytes[index + 1])?;
        out.push((high << 4) | low);
        index += 2;
    }
    Ok(out)
}

pub fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn extend_document(raw: &[u8]) -> Result<Value, SealFault> {
    jsonb_doc::bson_to_extended_json(raw).map_err(from_jsonb)
}

pub fn weight_token(extended: &Value) -> Result<String, SealFault> {
    let weight = extended.get("weight").ok_or_else(|| {
        fault(
            "field.missing",
            "/weight",
            "approved weight is absent from the document",
        )
    })?;
    if weight.is_number() {
        return Err(fault(
            "float.rejected",
            "/weight",
            "weight is a bare number and is not $numberDouble",
        ));
    }
    weight
        .get("$numberDouble")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            fault(
                "float.rejected",
                "/weight",
                "weight is not canonical $numberDouble",
            )
        })
}

pub fn weight_bits_match(token: &str) -> Result<u64, SealFault> {
    let parsed = token.parse::<f64>().map_err(|_| {
        fault(
            "float.rejected",
            "/weight",
            "canonical double does not parse",
        )
    })?;
    let bits = parsed.to_bits();
    if jsonb_doc::canonical_number_double(bits) != token {
        return Err(fault(
            "float.rejected",
            "/weight",
            "canonical double does not round-trip",
        ));
    }
    Ok(bits)
}

pub fn document_envelope(raw: &[u8], pg: &[u8], pg_text: &str) -> Value {
    json!({
        "protocol": jsonb_doc::PROTOCOL,
        "dataset": "oo_w141.shipments",
        "column": "doc",
        "oid": jsonb_doc::JSONB_OID,
        "typmod": jsonb_doc::JSONB_TYPMOD,
        "source_type": "jsonb",
        "raw_bson_dropped": false,
        "raw_bson_hex": encode_hex(raw),
        "raw_bson_sha256": payload_sha256(raw),
        "pg_jsonb_hex": encode_hex(pg),
        "pg_jsonb_text": pg_text,
        "loss_concealed": false,
        "missing_is_null": false,
        "approved_fields": APPROVED_FIELDS,
    })
}

pub fn logical_jsonb_file(extended: &Value) -> Result<(String, Vec<u8>), SealFault> {
    let text = serde_json::to_string(extended).map_err(|_| {
        fault(
            "schema.invalid",
            "/pg_jsonb_text",
            "extended JSON could not be written",
        )
    })?;
    let mut bytes = Vec::with_capacity(1 + text.len());
    bytes.push(jsonb_doc::JSONB_SEND_VERSION);
    bytes.extend(text.as_bytes());
    Ok((text, bytes))
}

pub fn seal_value(envelope: &Value) -> Result<Value, SealFault> {
    let text = serde_json::to_string(envelope).map_err(|_| {
        fault(
            "schema.invalid",
            "/",
            "document envelope could not be written",
        )
    })?;
    let sealed = jsonb_doc::seal_bson_jsonb_document(&text).map_err(from_jsonb)?;
    let weight = weight_token(&sealed.extended_json)?;
    let _bits = weight_bits_match(&weight)?;
    if sealed.oid != jsonb_doc::JSONB_OID || sealed.typmod != jsonb_doc::JSONB_TYPMOD {
        return Err(fault(
            "typmod.rejected",
            "/oid",
            "sealed document is not jsonb 3802 typmod -1",
        ));
    }
    if sealed.extended_json.get("weight").is_none() {
        return Err(fault(
            "field.missing",
            "/weight",
            "seal dropped approved weight",
        ));
    }
    Ok(json!({
        "protocol": PROTOCOL,
        "outcome": "sealed",
        "source_acceptance": false,
        "generation_created": false,
        "import_invoked": false,
        "model_called": false,
        "dataset": sealed.dataset,
        "column": sealed.column,
        "oid": sealed.oid,
        "typmod": sealed.typmod,
        "double_encoding": sealed.metadata.get("oo.document.double").cloned().unwrap_or_default(),
        "weight": weight,
        "raw_bson_sha256": sealed.raw_bson_sha256,
        "pg_jsonb_sha256": sealed.pg_jsonb_sha256,
        "identity_hash": sealed.identity_hash,
        "extended_json": sealed.extended_json,
        "field_scope": sealed.metadata.get("oo.document.field_scope").cloned().unwrap_or_default(),
    }))
}

pub fn profile_document(catalog_json: &str, envelope: &Value) -> Result<Value, SealFault> {
    let document_json = serde_json::to_string(envelope).map_err(|_| {
        fault(
            "schema.invalid",
            "/",
            "document envelope could not be written",
        )
    })?;
    let profile =
        compile_jsonb_document_profile(catalog_json, &document_json).map_err(|err| SealFault {
            code: err.code,
            path: err.path,
            message: err.message,
        })?;
    if profile.compiler != "compile_jsonb_document_profile" {
        return Err(fault(
            "schema.invalid",
            "/compiler",
            "profile compiler is not the fixed jsonb profile",
        ));
    }
    if !profile.document.contains(DOUBLE_ENCODING) {
        return Err(fault(
            "float.rejected",
            "/double",
            "profile does not name canonical_number_double",
        ));
    }
    Ok(json!({
        "protocol": PROTOCOL,
        "outcome": "profiled",
        "source_acceptance": false,
        "ready": false,
        "generation_created": false,
        "import_invoked": false,
        "compiler": profile.compiler,
        "dataset": profile.dataset,
        "column": profile.column,
        "document_identity_hash": profile.document_identity_hash,
        "schema_digest": profile.schema_digest,
        "raw_bson_sha256": profile.raw_bson_sha256,
        "pg_jsonb_sha256": profile.pg_jsonb_sha256,
        "document": profile.document,
    }))
}

pub fn set_root_string(raw: &[u8], key: &str, text: &str) -> Result<Vec<u8>, SealFault> {
    let mut payload = (i32::try_from(text.len() + 1).map_err(|_| {
        fault(
            "schema.invalid",
            "/set",
            "string value is outside the BSON length",
        )
    })?)
    .to_le_bytes()
    .to_vec();
    payload.extend(text.as_bytes());
    payload.push(0);
    set_root(raw, key, 0x02, &payload)
}

pub fn set_root_int32(raw: &[u8], key: &str, value: i32) -> Result<Vec<u8>, SealFault> {
    set_root(raw, key, 0x10, &value.to_le_bytes())
}

pub fn importer_request(envelope: &Value) -> Result<Value, SealFault> {
    let catalog = probe_catalog()?;
    sealed_import::parse_sealed_import_catalog(&catalog.to_string()).map_err(from_import)?;
    Ok(json!({
        "protocol": IMPORTER_PROTOCOL,
        "engine": "mongodb",
        "source_id": "oo_w141",
        "catalog": catalog,
        "document": envelope,
        "source_acceptance": false,
        "generation_created": false,
        "import_invoked": false,
    }))
}

pub fn decode_report(raw: &[u8]) -> Result<Value, SealFault> {
    let extended = extend_document(raw)?;
    let bson_types: serde_json::Map<String, Value> = split_document(raw)?
        .into_iter()
        .map(|element| {
            let mut observed = json!({"variant": format!("{:02x}", element.kind)});
            if element.kind == 0x05 {
                observed["subtype"] = json!(format!("{:02x}", element.payload[4]));
            }
            (element.key, observed)
        })
        .collect();
    let weight = weight_token(&extended)?;
    let bits = weight_bits_match(&weight)?;
    let keys = object_keys(&extended);
    if keys
        .iter()
        .any(|key| !APPROVED_FIELDS.contains(&key.as_str()))
    {
        return Ok(json!({
            "protocol": PROTOCOL,
            "outcome": "scope_refused",
            "source_acceptance": false,
            "code": format!("{}/field.scope", jsonb_doc::PROTOCOL),
            "weight": weight,
            "weight_bits": format!("{bits:016x}"),
            "keys": keys,
            "raw_bson_sha256": payload_sha256(raw),
            "bson_types": bson_types,
            "extended_json": extended,
            "raw_doc_authorized": false,
        }));
    }
    Ok(json!({
        "protocol": PROTOCOL,
        "outcome": "decoded",
        "source_acceptance": false,
        "weight": weight,
        "weight_encoding": "$numberDouble",
        "weight_bits": format!("{bits:016x}"),
        "keys": keys,
        "raw_bson_sha256": payload_sha256(raw),
        "bson_types": bson_types,
        "extended_json": extended,
        "presence_keys_in_document": false,
    }))
}

fn probe_catalog() -> Result<Value, SealFault> {
    let mut preimage = String::new();
    push_schema_table(&mut preimage, "public", "docs");
    push_schema_column(&mut preimage, "id", 23, -1, None);
    push_schema_column(
        &mut preimage,
        "doc",
        jsonb_doc::JSONB_OID,
        jsonb_doc::JSONB_TYPMOD,
        None,
    );
    let schema_digest = payload_sha256(preimage.as_bytes());
    let digest = "a".repeat(64);
    let mark = "a1";
    Ok(json!({
        "capture": "sealed_import",
        "config_digest": digest,
        "engine": "mongodb",
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
                "physical_name": shadow("pt_", mark, 1),
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
        "source_id": "oo_w141",
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
                    "stored_physical": shadow("pc_", mark, 2),
                    "typmod": -1
                },
                {
                    "caveat": null,
                    "key": false,
                    "logical_name": "doc",
                    "oid": jsonb_doc::JSONB_OID,
                    "source_type": "jsonb",
                    "stored_physical": shadow("pc_", mark, 3),
                    "typmod": jsonb_doc::JSONB_TYPMOD
                }
            ],
            "logical_schema": "public",
            "logical_table": "docs",
            "rejects": [],
            "stored_namespace": shadow("pn_", mark, 0),
            "stored_physical": shadow("pt_", mark, 1)
        }],
        "target_generation": 1,
        "tool_hook_digest": digest,
        "type_digest": digest
    }))
}

fn shadow(prefix: &str, mark: &str, salt: u8) -> String {
    let hex = format!("{mark}{salt:02x}");
    format!(
        "{prefix}{}",
        hex.chars().cycle().take(60).collect::<String>()
    )
}

fn set_root(raw: &[u8], key: &str, kind: u8, payload: &[u8]) -> Result<Vec<u8>, SealFault> {
    if key.is_empty() || key.contains('\0') {
        return Err(fault(
            "schema.invalid",
            "/set",
            "BSON key is empty or contains NUL",
        ));
    }
    let mut elements = split_document(raw)?;
    let next = Element {
        kind,
        key: key.to_string(),
        payload: payload.to_vec(),
    };
    if let Some(found) = elements.iter_mut().find(|item| item.key == key) {
        *found = next;
    } else {
        elements.push(next);
    }
    join_document(&elements)
}

fn split_document(raw: &[u8]) -> Result<Vec<Element>, SealFault> {
    if raw.len() < 5 {
        return Err(fault(
            "pg.truncated",
            "/raw_bson_hex",
            "BSON document is shorter than a header",
        ));
    }
    let declared = read_i32(raw, 0)?;
    if declared as usize != raw.len() || raw[raw.len() - 1] != 0 {
        return Err(fault(
            "pg.truncated",
            "/raw_bson_hex",
            "BSON document length does not match the buffer",
        ));
    }
    let mut cursor = 4;
    let end = raw.len() - 1;
    let mut elements = Vec::new();
    while cursor < end {
        let kind = raw[cursor];
        cursor += 1;
        let key_end = raw[cursor..end]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| {
                fault(
                    "pg.truncated",
                    "/raw_bson_hex",
                    "BSON key is not terminated",
                )
            })?;
        let key = std::str::from_utf8(&raw[cursor..cursor + key_end])
            .map_err(|_| fault("bson.utf8", "/raw_bson_hex", "BSON key is not utf-8"))?;
        cursor += key_end + 1;
        let (payload, size) = read_payload(kind, &raw[cursor..end])?;
        cursor += size;
        elements.push(Element {
            kind,
            key: key.to_string(),
            payload,
        });
    }
    if cursor != end {
        return Err(fault(
            "pg.truncated",
            "/raw_bson_hex",
            "BSON document has trailing bytes",
        ));
    }
    Ok(elements)
}

fn read_payload(kind: u8, bytes: &[u8]) -> Result<(Vec<u8>, usize), SealFault> {
    let size = match kind {
        0x01 | 0x09 | 0x12 => 8,
        0x07 => 12,
        0x08 => 1,
        0x0A => 0,
        0x10 => 4,
        0x13 => 16,
        0x02 => string_size(bytes)?,
        0x03 | 0x04 => document_size(bytes)?,
        0x05 => binary_size(bytes)?,
        _ => {
            return Err(fault(
                "type.unsupported",
                "/raw_bson_hex",
                "BSON type is not in the shipment document",
            ));
        }
    };
    if size > bytes.len() {
        return Err(fault(
            "pg.truncated",
            "/raw_bson_hex",
            "BSON value is truncated",
        ));
    }
    Ok((bytes[..size].to_vec(), size))
}

fn string_size(bytes: &[u8]) -> Result<usize, SealFault> {
    let declared = read_i32(bytes, 0)?;
    if declared < 1 {
        return Err(fault(
            "pg.truncated",
            "/raw_bson_hex",
            "BSON string size is empty",
        ));
    }
    let data = usize::try_from(declared).map_err(|_| {
        fault(
            "pg.truncated",
            "/raw_bson_hex",
            "BSON string size is outside the buffer",
        )
    })?;
    Ok(4 + data)
}

fn document_size(bytes: &[u8]) -> Result<usize, SealFault> {
    let declared = read_i32(bytes, 0)?;
    if declared < 5 {
        return Err(fault(
            "pg.truncated",
            "/raw_bson_hex",
            "BSON subdocument is shorter than a header",
        ));
    }
    usize::try_from(declared).map_err(|_| {
        fault(
            "pg.truncated",
            "/raw_bson_hex",
            "BSON subdocument size is outside the buffer",
        )
    })
}

fn binary_size(bytes: &[u8]) -> Result<usize, SealFault> {
    let declared = read_i32(bytes, 0)?;
    if declared < 0 {
        return Err(fault(
            "pg.truncated",
            "/raw_bson_hex",
            "BSON binary size is negative",
        ));
    }
    let data = usize::try_from(declared).map_err(|_| {
        fault(
            "pg.truncated",
            "/raw_bson_hex",
            "BSON binary size is outside the buffer",
        )
    })?;
    Ok(4 + 1 + data)
}

fn join_document(elements: &[Element]) -> Result<Vec<u8>, SealFault> {
    let mut body = Vec::new();
    for element in elements {
        body.push(element.kind);
        body.extend(element.key.as_bytes());
        body.push(0);
        body.extend(&element.payload);
    }
    body.push(0);
    let len = i32::try_from(body.len() + 4).map_err(|_| {
        fault(
            "schema.invalid",
            "/raw_bson_hex",
            "BSON document is outside the length",
        )
    })?;
    let mut out = len.to_le_bytes().to_vec();
    out.extend(body);
    Ok(out)
}

fn read_i32(bytes: &[u8], at: usize) -> Result<i32, SealFault> {
    let end = at
        .checked_add(4)
        .ok_or_else(|| fault("pg.truncated", "/raw_bson_hex", "BSON integer is truncated"))?;
    if end > bytes.len() {
        return Err(fault(
            "pg.truncated",
            "/raw_bson_hex",
            "BSON integer is truncated",
        ));
    }
    let mut buf = [0; 4];
    buf.copy_from_slice(&bytes[at..end]);
    Ok(i32::from_le_bytes(buf))
}

fn object_keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

fn hex_nibble(byte: u8) -> Result<u8, SealFault> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(fault(
            "schema.invalid",
            "/raw_bson_hex",
            "raw BSON hex is not hexadecimal",
        )),
    }
}

fn from_jsonb(err: jsonb_doc::JsonbDocError) -> SealFault {
    SealFault {
        code: err.code,
        path: err.path,
        message: err.message,
    }
}

fn from_import(err: sealed_import::SealedImportError) -> SealFault {
    SealFault {
        code: err.code,
        path: err.path,
        message: err.message,
    }
}

fn fault(name: &'static str, path: &'static str, message: &'static str) -> SealFault {
    SealFault {
        code: format!("{}/{}", jsonb_doc::PROTOCOL, name),
        path: path.to_string(),
        message,
    }
}

pub fn fault_json(err: &SealFault) -> Value {
    json!({
        "protocol": PROTOCOL,
        "outcome": "refused",
        "source_acceptance": false,
        "generation_created": false,
        "import_invoked": false,
        "ready": false,
        "raw_doc_authorized": false,
        "code": err.code,
        "path": err.path,
        "message": err.message,
    })
}
