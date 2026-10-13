//! BSON document sealed as PostgreSQL jsonb OID 3802.
//!
//! The storage type is jsonb, not json OID 114. A full document is sealed only
//! when the raw BSON bytes are present. Missing object keys stay absent; JSON
//! null is a present value. Int64 and Decimal128 stay exact strings and are
//! not rendered through `f64`. A BSON double is Canonical Extended JSON
//! `$numberDouble`: finite values keep a round-trip decimal, negative zero is
//! the string `-0.0`, and non-finite values are `NaN`, `Infinity`, or
//! `-Infinity`. NaN payloads and the NaN sign bit are not preserved. This
//! module does not open a database.
//!
//! PostgreSQL 17.10 `jsonb_send` on the pinned arm64 image is one version byte
//! `1` followed by the `jsonb` text. `json_send` is that text without the
//! version byte and is refused.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Map, Number, Value};

use crate::api::payload_sha256;
use crate::decimal::ExactDecimal;
use crate::json_scan::parse_json_value;

/// Document seal protocol. This is the fixed interface for task 41 and task 06.
pub const PROTOCOL: &str = "oo.document.bson-jsonb/v1";

/// Builder profile protocol emitted from a sealed document.
pub const PROFILE_PROTOCOL: &str = "oo.builder.document-jsonb-profile/v1";

pub const JSONB_OID: i64 = 3802;
pub const JSONB_TYPMOD: i32 = -1;
pub const JSON_OID: i64 = 114;
pub const JSONB_SEND_VERSION: u8 = 1;
pub const ARROW_TYPE: &str = "utf8";

/// BSON double encoding named by the seal, the profile, and the role page.
pub const DOUBLE_ENCODING: &str = "canonical_number_double";
/// Canonical string for IEEE negative zero. A JSON number cannot keep the sign.
pub const DOUBLE_NEGATIVE_ZERO: &str = "-0.0";
/// Canonical strings for the three non-finite classes.
pub const DOUBLE_NON_FINITE: &str = "NaN|Infinity|-Infinity";
/// Every NaN payload and the NaN sign bit become the single string `NaN`.
pub const DOUBLE_NAN_PAYLOAD: &str = "not_preserved";
/// Bare jsonb numbers must be exact decimals. Int64 and Decimal128 do not use `f64`.
pub const FLOAT_PRECISION: &str = "rejected";
/// Root keys were checked against `approved_fields`.
pub const FIELD_SCOPE_APPROVED: &str = "approved_root_keys";
/// No approved-field list was supplied, so root keys were not filtered.
pub const FIELD_SCOPE_UNCHECKED: &str = "unchecked";

const MAX_BYTES: usize = 1_048_576;
const MAX_DEPTH: usize = 16;
const DECIMAL_BIAS: i32 = 6176;
const COEFF_LIMIT: u128 = 10_000_000_000_000_000_000_000_000_000_000_000;

/// Failure of a document seal or a jsonb file check. The message does not echo payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonbDocError {
    pub code: String,
    pub path: String,
    pub message: &'static str,
}

impl JsonbDocError {
    fn new(name: &'static str, path: impl Into<String>, message: &'static str) -> Self {
        Self {
            code: format!("{PROTOCOL}/{name}"),
            path: path.into(),
            message,
        }
    }
}

impl std::fmt::Display for JsonbDocError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} at {}: {}",
            self.code, self.path, self.message
        )
    }
}

impl std::error::Error for JsonbDocError {}

/// One PostgreSQL `jsonb_send` file. `text` is the bytes after the version byte.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonbFile {
    pub version: u8,
    pub text: String,
    pub value: Value,
}

/// Sealed full document. Arrow metadata is the storage and quality contract.
#[derive(Debug, Clone, PartialEq)]
pub struct SealedJsonbDocument {
    pub dataset: String,
    pub column: String,
    pub oid: i64,
    pub typmod: i32,
    pub source_type: Option<String>,
    pub arrow_type: &'static str,
    pub metadata: BTreeMap<String, String>,
    pub extended_json: Value,
    pub raw_bson_sha256: String,
    pub pg_jsonb_sha256: String,
    pub identity_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentIn {
    protocol: String,
    dataset: String,
    column: String,
    oid: i64,
    typmod: i32,
    #[serde(default)]
    source_type: Option<String>,
    #[serde(default)]
    raw_bson_dropped: bool,
    raw_bson_hex: String,
    #[serde(default)]
    raw_bson_sha256: Option<String>,
    pg_jsonb_hex: String,
    pg_jsonb_text: String,
    loss_concealed: bool,
    missing_is_null: bool,
    /// Root keys allowed in the raw document. Absent means the caller did not
    /// ask for a scope check. Present keys outside this list are refused.
    #[serde(default)]
    approved_fields: Option<Vec<String>>,
}

/// Canonical Extended JSON `$numberDouble` string for one IEEE-754 binary64.
///
/// Finite values use the shortest round-trip decimal. A finite string with no
/// decimal point and no exponent gains `.0`. Negative zero is `-0.0`.
/// Non-finite values are `NaN`, `Infinity`, and `-Infinity`.
pub fn canonical_number_double(bits: u64) -> String {
    let value = f64::from_bits(bits);
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            DOUBLE_NEGATIVE_ZERO.to_string()
        } else {
            "0.0".to_string()
        };
    }
    let mut text = value.to_string();
    if !text.contains('.') && !text.contains('e') && !text.contains('E') {
        text.push_str(".0");
    }
    text
}

/// Decode one raw BSON document to Canonical Extended JSON.
///
/// This is the value the PostgreSQL jsonb file must match. It does not seal
/// and it does not apply an approved-field list.
pub fn bson_to_extended_json(raw: &[u8]) -> Result<Value, JsonbDocError> {
    decode_bson_document(raw)
}

/// Read one `jsonb_send` file. JSON text without the version byte is refused.
pub fn inspect_jsonb_send(bytes: &[u8]) -> Result<JsonbFile, JsonbDocError> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(JsonbDocError::new(
            "pg.truncated",
            "/pg_jsonb",
            "jsonb file is empty or larger than the document budget",
        ));
    }
    if bytes[0] != JSONB_SEND_VERSION {
        if matches!(
            bytes[0],
            b'{' | b'[' | b'"' | b't' | b'f' | b'n' | b'-' | b'0'..=b'9'
        ) {
            return Err(JsonbDocError::new(
                "json_text_not_jsonb",
                "/pg_jsonb",
                "json text send is not jsonb",
            ));
        }
        return Err(JsonbDocError::new(
            "pg.version",
            "/pg_jsonb",
            "jsonb send version is not 1",
        ));
    }
    let text = std::str::from_utf8(&bytes[1..]).map_err(|_| {
        JsonbDocError::new(
            "pg.truncated",
            "/pg_jsonb",
            "jsonb send payload is not utf-8 text",
        )
    })?;
    let value = parse_json_value(text, PROTOCOL).map_err(|err| {
        let duplicate = err
            .items
            .iter()
            .any(|item| item.code.ends_with("/json.duplicate_key"));
        if duplicate {
            JsonbDocError::new("duplicate_key", "/pg_jsonb", "jsonb text repeats an object key")
        } else {
            JsonbDocError::new(
                "pg.truncated",
                "/pg_jsonb",
                "jsonb send payload is not one JSON value",
            )
        }
    })?;
    reject_inexact(&value)?;
    Ok(JsonbFile {
        version: JSONB_SEND_VERSION,
        text: text.to_string(),
        value,
    })
}

/// Seal one full BSON document into the jsonb 3802 Arrow contract.
///
/// `raw_bson_hex` is required. An empty buffer, a dropped-buffer flag, a hash
/// that does not match the bytes, OID 114, a typmod other than -1, concealed
/// loss, or a missing-key-as-null claim is refused.
pub fn seal_bson_jsonb_document(input_json: &str) -> Result<SealedJsonbDocument, JsonbDocError> {
    let parsed = parse_json_value(input_json, PROTOCOL).map_err(|err| {
        let duplicate = err
            .items
            .iter()
            .any(|item| item.code.ends_with("/json.duplicate_key"));
        if duplicate {
            JsonbDocError::new("duplicate_key", "/", "document envelope repeats an object key")
        } else {
            JsonbDocError::new("schema.invalid", "/", "document envelope is not JSON")
        }
    })?;
    if let Some(field) = parsed.get("approved_fields")
        && !field.is_array()
    {
        return Err(JsonbDocError::new(
            "schema.invalid",
            "/approved_fields",
            "approved field list is not an array",
        ));
    }
    let input: DocumentIn = serde_json::from_value(parsed).map_err(|err| {
        let text = err.to_string();
        if text.contains("unknown field") {
            JsonbDocError::new("field.unknown", "/", "document envelope has an unknown field")
        } else if text.contains("missing field") {
            JsonbDocError::new("field.missing", "/", "document envelope is missing a field")
        } else {
            JsonbDocError::new("schema.invalid", "/", "document envelope is not the sealed shape")
        }
    })?;
    if input.protocol != PROTOCOL {
        return Err(JsonbDocError::new(
            "protocol.mismatch",
            "/protocol",
            "document protocol is not oo.document.bson-jsonb/v1",
        ));
    }
    if input.oid == JSON_OID {
        return Err(JsonbDocError::new(
            "json114_not_jsonb",
            "/oid",
            "json oid 114 is not jsonb",
        ));
    }
    if input.oid != JSONB_OID {
        return Err(JsonbDocError::new(
            "oid.rejected",
            "/oid",
            "document storage oid is not 3802",
        ));
    }
    if input.typmod != JSONB_TYPMOD {
        return Err(JsonbDocError::new(
            "typmod.rejected",
            "/typmod",
            "jsonb typmod is not -1",
        ));
    }
    if input.source_type.as_deref().is_some_and(|token| token != "jsonb") {
        return Err(JsonbDocError::new(
            "source_type.rejected",
            "/source_type",
            "source type token is not jsonb",
        ));
    }
    if !closed_dataset(&input.dataset) || !closed_ident(&input.column) {
        return Err(JsonbDocError::new(
            "schema.invalid",
            "/dataset",
            "dataset or column is not a closed identifier",
        ));
    }
    if input.loss_concealed {
        return Err(JsonbDocError::new(
            "loss_concealed",
            "/loss_concealed",
            "concealed loss is not accepted",
        ));
    }
    if input.missing_is_null {
        return Err(JsonbDocError::new(
            "missing_is_null",
            "/missing_is_null",
            "a missing key is not JSON null",
        ));
    }
    if input.raw_bson_dropped || input.raw_bson_hex.is_empty() {
        return Err(JsonbDocError::new(
            "raw_bson_dropped",
            "/raw_bson_hex",
            "full document raw BSON was dropped",
        ));
    }
    if let Some(names) = &input.approved_fields {
        validate_approved(names)?;
    }
    let raw = decode_hex(&input.raw_bson_hex, "/raw_bson_hex")?;
    let pg_bytes = decode_hex(&input.pg_jsonb_hex, "/pg_jsonb_hex")?;
    let raw_hash = payload_sha256(&raw);
    let pg_hash = payload_sha256(&pg_bytes);
    if input
        .raw_bson_sha256
        .as_ref()
        .is_some_and(|claimed| claimed != &raw_hash)
    {
        return Err(JsonbDocError::new(
            "digest.mismatch",
            "/raw_bson_sha256",
            "raw BSON hash does not match the bytes",
        ));
    }
    let extended = decode_bson_document(&raw)?;
    if !extended.is_object() {
        return Err(JsonbDocError::new(
            "root.not_object",
            "/raw_bson_hex",
            "BSON root is not a document",
        ));
    }
    if let Some(names) = &input.approved_fields {
        enforce_scope(&extended, names)?;
    }
    let file = inspect_jsonb_send(&pg_bytes)?;
    if file.text != input.pg_jsonb_text {
        return Err(JsonbDocError::new(
            "pg.text_mismatch",
            "/pg_jsonb_text",
            "jsonb text does not match the send bytes",
        ));
    }
    if !file.value.is_object() || !values_eq(&file.value, &extended) {
        return Err(JsonbDocError::new(
            "pg.mismatch",
            "/pg_jsonb_hex",
            "jsonb file does not match the BSON document",
        ));
    }
    let identity_hash = payload_sha256(
        format!(
            "oo.document.bson-jsonb/v1\n{}\n{}\n{}\n{}\n{raw_hash}\n{pg_hash}\n",
            input.dataset, input.column, input.oid, input.typmod
        )
        .as_bytes(),
    );
    let mut metadata = BTreeMap::new();
    metadata.insert("oo.arrow.type".to_string(), ARROW_TYPE.to_string());
    metadata.insert("oo.storage.type".to_string(), "jsonb".to_string());
    metadata.insert("oo.storage.oid".to_string(), JSONB_OID.to_string());
    metadata.insert("oo.storage.typmod".to_string(), JSONB_TYPMOD.to_string());
    metadata.insert(
        "oo.document.kind".to_string(),
        "bson_extended_json".to_string(),
    );
    metadata.insert("oo.document.raw_bson_sha256".to_string(), raw_hash.clone());
    metadata.insert("oo.document.pg_jsonb_sha256".to_string(), pg_hash.clone());
    metadata.insert(
        "oo.document.missing".to_string(),
        "absent_not_null".to_string(),
    );
    metadata.insert(
        "oo.document.uuid".to_string(),
        "rfc4122_lowercase".to_string(),
    );
    metadata.insert(
        "oo.document.date".to_string(),
        "unix_millis_utc".to_string(),
    );
    metadata.insert(
        "oo.document.bytes".to_string(),
        "base64_subtype".to_string(),
    );
    metadata.insert(
        "oo.document.decimal".to_string(),
        "exact_decimal_string".to_string(),
    );
    metadata.insert(
        "oo.document.double".to_string(),
        DOUBLE_ENCODING.to_string(),
    );
    metadata.insert(
        "oo.document.double.negative_zero".to_string(),
        DOUBLE_NEGATIVE_ZERO.to_string(),
    );
    metadata.insert(
        "oo.document.double.non_finite".to_string(),
        DOUBLE_NON_FINITE.to_string(),
    );
    metadata.insert(
        "oo.document.double.nan_payload".to_string(),
        DOUBLE_NAN_PAYLOAD.to_string(),
    );
    metadata.insert(
        "oo.document.field_scope".to_string(),
        if input.approved_fields.is_some() {
            FIELD_SCOPE_APPROVED.to_string()
        } else {
            FIELD_SCOPE_UNCHECKED.to_string()
        },
    );
    metadata.insert("oo.quality.loss_concealed".to_string(), "false".to_string());
    metadata.insert(
        "oo.precision.float".to_string(),
        FLOAT_PRECISION.to_string(),
    );
    Ok(SealedJsonbDocument {
        dataset: input.dataset,
        column: input.column,
        oid: input.oid,
        typmod: input.typmod,
        source_type: input.source_type,
        arrow_type: ARROW_TYPE,
        metadata,
        extended_json: extended,
        raw_bson_sha256: raw_hash,
        pg_jsonb_sha256: pg_hash,
        identity_hash,
    })
}

fn reject_inexact(value: &Value) -> Result<(), JsonbDocError> {
    match value {
        Value::Number(number) => exact_number(number),
        Value::Array(items) => items.iter().try_for_each(reject_inexact),
        Value::Object(map) => map.values().try_for_each(reject_inexact),
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(()),
    }
}

fn exact_number(number: &Number) -> Result<(), JsonbDocError> {
    let lexeme = number.to_string();
    if lexeme.bytes().any(|byte| matches!(byte, b'e' | b'E' | b'+'))
        || ExactDecimal::parse_sql(&lexeme).is_err()
    {
        return Err(JsonbDocError::new(
            "float.rejected",
            "/pg_jsonb",
            "jsonb number is not an exact decimal",
        ));
    }
    Ok(())
}

fn values_eq(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::String(left), Value::String(right)) => left == right,
        (Value::Number(left), Value::Number(right)) => left.to_string() == right.to_string(),
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|(left, right)| values_eq(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, value)| {
                    right
                        .get(key)
                        .is_some_and(|other| values_eq(value, other))
                })
        }
        _ => false,
    }
}

fn decode_hex(text: &str, path: &str) -> Result<Vec<u8>, JsonbDocError> {
    if text.is_empty()
        || text.len() > MAX_BYTES * 2
        || !text.len().is_multiple_of(2)
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(JsonbDocError::new(
            "schema.invalid",
            path,
            "hex payload is not lowercase even-length hex",
        ));
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let pair = std::str::from_utf8(&bytes[index..index + 2]).unwrap_or("");
        let value = u8::from_str_radix(pair, 16).map_err(|_| {
            JsonbDocError::new("schema.invalid", path, "hex payload is not lowercase even-length hex")
        })?;
        out.push(value);
        index += 2;
    }
    Ok(out)
}

fn decode_bson_document(bytes: &[u8]) -> Result<Value, JsonbDocError> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(JsonbDocError::new(
            "budget.exceeded",
            "/raw_bson_hex",
            "raw BSON is empty or larger than the document budget",
        ));
    }
    let (value, used) = read_document(bytes, 1)?;
    if used != bytes.len() {
        return Err(JsonbDocError::new(
            "pg.truncated",
            "/raw_bson_hex",
            "raw BSON has trailing bytes",
        ));
    }
    Ok(value)
}

fn read_document(bytes: &[u8], depth: usize) -> Result<(Value, usize), JsonbDocError> {
    if depth > MAX_DEPTH {
        return Err(JsonbDocError::new(
            "depth.exceeded",
            "/raw_bson_hex",
            "BSON nesting exceeds 16",
        ));
    }
    if bytes.len() < 5 {
        return Err(truncated());
    }
    let declared = read_i32(bytes)?;
    if declared < 5 {
        return Err(truncated());
    }
    let declared = declared as usize;
    if declared > bytes.len() || bytes[declared - 1] != 0 {
        return Err(truncated());
    }
    let mut index = 4;
    let end = declared - 1;
    let mut map = Map::new();
    while index < end {
        let kind = bytes[index];
        index += 1;
        let key_end = bytes[index..end]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(truncated)?;
        let key = std::str::from_utf8(&bytes[index..index + key_end]).map_err(|_| {
            JsonbDocError::new("bson.utf8", "/raw_bson_hex", "BSON key is not utf-8")
        })?;
        index += key_end + 1;
        let (value, used) = read_value(kind, &bytes[index..declared], depth)?;
        index += used;
        if index > end {
            return Err(truncated());
        }
        if map.contains_key(key) {
            return Err(JsonbDocError::new(
                "duplicate_key",
                "/raw_bson_hex",
                "BSON document repeats a key",
            ));
        }
        map.insert(key.to_string(), value);
    }
    if index != end {
        return Err(truncated());
    }
    Ok((Value::Object(map), declared))
}

fn read_value(kind: u8, bytes: &[u8], depth: usize) -> Result<(Value, usize), JsonbDocError> {
    match kind {
        0x01 => read_double(bytes),
        0x02 => read_string(bytes),
        0x03 => read_document(bytes, depth + 1),
        0x04 => read_array(bytes, depth + 1),
        0x05 => read_binary(bytes),
        0x07 => read_object_id(bytes),
        0x08 => read_bool(bytes),
        0x09 => read_datetime(bytes),
        0x0A => Ok((Value::Null, 0)),
        0x10 => read_int32(bytes),
        0x12 => read_int64(bytes),
        0x13 => read_decimal128(bytes),
        _ => Err(JsonbDocError::new(
            "type.unsupported",
            "/raw_bson_hex",
            "BSON type is not in the sealed document subset",
        )),
    }
}

fn read_double(bytes: &[u8]) -> Result<(Value, usize), JsonbDocError> {
    if bytes.len() < 8 {
        return Err(truncated());
    }
    let bits = u64::from_le_bytes(copy8(bytes));
    let text = canonical_number_double(bits);
    json_string_object(&[("$numberDouble", &text)]).map(|value| (value, 8))
}

fn validate_approved(names: &[String]) -> Result<(), JsonbDocError> {
    let mut seen = BTreeMap::<&str, ()>::new();
    for name in names {
        if !approved_name(name) || seen.insert(name.as_str(), ()).is_some() {
            return Err(JsonbDocError::new(
                "schema.invalid",
                "/approved_fields",
                "approved field list is not a set of closed names",
            ));
        }
    }
    Ok(())
}

fn approved_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn enforce_scope(document: &Value, approved: &[String]) -> Result<(), JsonbDocError> {
    let Some(map) = document.as_object() else {
        return Err(JsonbDocError::new(
            "root.not_object",
            "/raw_bson_hex",
            "BSON root is not a document",
        ));
    };
    let allowed: BTreeMap<&str, ()> = approved.iter().map(|name| (name.as_str(), ())).collect();
    if map.keys().any(|key| !allowed.contains_key(key.as_str())) {
        return Err(JsonbDocError::new(
            "field.scope",
            "/raw_bson_hex",
            "raw document key is outside the approved field scope",
        ));
    }
    Ok(())
}

fn read_string(bytes: &[u8]) -> Result<(Value, usize), JsonbDocError> {
    if bytes.len() < 5 {
        return Err(truncated());
    }
    let declared = read_i32(bytes)?;
    if declared < 1 {
        return Err(truncated());
    }
    let declared = declared as usize;
    if declared > bytes.len() - 4 || bytes[4 + declared - 1] != 0 {
        return Err(truncated());
    }
    let text = std::str::from_utf8(&bytes[4..4 + declared - 1]).map_err(|_| {
        JsonbDocError::new("bson.utf8", "/raw_bson_hex", "BSON string is not utf-8")
    })?;
    Ok((Value::String(text.to_string()), 4 + declared))
}

fn read_array(bytes: &[u8], depth: usize) -> Result<(Value, usize), JsonbDocError> {
    let (value, used) = read_document(bytes, depth)?;
    let Some(map) = value.as_object() else {
        return Err(truncated());
    };
    let mut items = Vec::with_capacity(map.len());
    for index in 0..map.len() {
        let key = index.to_string();
        let Some(item) = map.get(&key) else {
            return Err(JsonbDocError::new(
                "array.keys",
                "/raw_bson_hex",
                "BSON array keys are not 0, 1, 2, ...",
            ));
        };
        items.push(item.clone());
    }
    if items.len() != map.len() {
        return Err(JsonbDocError::new(
            "array.keys",
            "/raw_bson_hex",
            "BSON array keys are not 0, 1, 2, ...",
        ));
    }
    Ok((Value::Array(items), used))
}

fn read_binary(bytes: &[u8]) -> Result<(Value, usize), JsonbDocError> {
    if bytes.len() < 5 {
        return Err(truncated());
    }
    let declared = read_i32(bytes)?;
    if declared < 0 {
        return Err(truncated());
    }
    let declared = declared as usize;
    if bytes.len() < 5 + declared {
        return Err(truncated());
    }
    let subtype = bytes[4];
    let data = &bytes[5..5 + declared];
    let value = if subtype == 0x04 {
        if data.len() != 16 {
            return Err(JsonbDocError::new(
                "uuid.invalid",
                "/raw_bson_hex",
                "UUID subtype is not 16 bytes",
            ));
        }
        let mut uuid = [0_u8; 16];
        uuid.copy_from_slice(data);
        json_string_object(&[("$uuid", &format_uuid(&uuid)?)])?
    } else {
        let mut object = Map::new();
        object.insert("base64".to_string(), Value::String(base64_encode(data)));
        object.insert(
            "subType".to_string(),
            Value::String(format!("{subtype:02x}")),
        );
        let mut wrapped = Map::new();
        wrapped.insert("$binary".to_string(), Value::Object(object));
        Value::Object(wrapped)
    };
    Ok((value, 5 + declared))
}

fn read_object_id(bytes: &[u8]) -> Result<(Value, usize), JsonbDocError> {
    if bytes.len() < 12 {
        return Err(truncated());
    }
    json_string_object(&[("$oid", &hex_encode(&bytes[..12]))]).map(|value| (value, 12))
}

fn read_bool(bytes: &[u8]) -> Result<(Value, usize), JsonbDocError> {
    match bytes.first() {
        Some(0) => Ok((Value::Bool(false), 1)),
        Some(1) => Ok((Value::Bool(true), 1)),
        Some(_) => Err(JsonbDocError::new(
            "schema.invalid",
            "/raw_bson_hex",
            "BSON bool is not 0 or 1",
        )),
        None => Err(truncated()),
    }
}

fn read_datetime(bytes: &[u8]) -> Result<(Value, usize), JsonbDocError> {
    if bytes.len() < 8 {
        return Err(truncated());
    }
    let millis = i64::from_le_bytes(copy8(bytes));
    let mut inner = Map::new();
    inner.insert(
        "$numberLong".to_string(),
        Value::String(millis.to_string()),
    );
    let mut object = Map::new();
    object.insert("$date".to_string(), Value::Object(inner));
    Ok((Value::Object(object), 8))
}

fn read_int32(bytes: &[u8]) -> Result<(Value, usize), JsonbDocError> {
    if bytes.len() < 4 {
        return Err(truncated());
    }
    let value = i32::from_le_bytes(copy4(bytes));
    Ok((json_number(&value.to_string())?, 4))
}

fn read_int64(bytes: &[u8]) -> Result<(Value, usize), JsonbDocError> {
    if bytes.len() < 8 {
        return Err(truncated());
    }
    let value = i64::from_le_bytes(copy8(bytes));
    json_string_object(&[("$numberLong", &value.to_string())]).map(|value| (value, 8))
}

fn read_decimal128(bytes: &[u8]) -> Result<(Value, usize), JsonbDocError> {
    if bytes.len() < 16 {
        return Err(truncated());
    }
    let mut raw = [0_u8; 16];
    raw.copy_from_slice(&bytes[..16]);
    let text = format_decimal128(&raw)?;
    json_string_object(&[("$numberDecimal", &text)]).map(|value| (value, 16))
}

fn format_decimal128(bytes: &[u8; 16]) -> Result<String, JsonbDocError> {
    let low = u64::from_le_bytes(copy8(bytes));
    let high = u64::from_le_bytes(copy8(&bytes[8..]));
    let negative = high >> 63 == 1;
    let combination = (high >> 46) & 0x1_FFFF;
    if combination >= 0x1_F000 {
        return Err(JsonbDocError::new(
            "decimal.rejected",
            "/raw_bson_hex",
            "Decimal128 NaN is rejected",
        ));
    }
    if combination >= 0x1_E000 {
        return Err(JsonbDocError::new(
            "decimal.rejected",
            "/raw_bson_hex",
            "Decimal128 infinity is rejected",
        ));
    }
    let (biased, msd) = if combination < 0x1_8000 {
        (combination >> 3, combination & 0x7)
    } else {
        ((combination >> 1) & 0x3FFF, 8 + (combination & 1))
    };
    if msd > 9 {
        return Err(JsonbDocError::new(
            "decimal.rejected",
            "/raw_bson_hex",
            "Decimal128 coefficient is not exact",
        ));
    }
    let trailing = ((high & 0x0000_3FFF_FFFF_FFFF) as u128) << 64 | u128::from(low);
    let coeff = (u128::from(msd) << 110) | trailing;
    if coeff >= COEFF_LIMIT {
        return Err(JsonbDocError::new(
            "decimal.rejected",
            "/raw_bson_hex",
            "Decimal128 coefficient is not exact",
        ));
    }
    let exp = (biased as i32) - DECIMAL_BIAS;
    let scale = exp.checked_neg().ok_or_else(|| {
        JsonbDocError::new(
            "decimal.rejected",
            "/raw_bson_hex",
            "Decimal128 scale is outside the exact range",
        )
    })?;
    if coeff == 0 {
        let body = ExactDecimal::new(false, "0", scale)
            .map_err(|_| {
                JsonbDocError::new(
                    "decimal.rejected",
                    "/raw_bson_hex",
                    "Decimal128 scale is outside the exact range",
                )
            })?
            .to_string();
        return Ok(if negative { format!("-{body}") } else { body });
    }
    let digits = coeff.to_string();
    ExactDecimal::new(negative, digits, scale)
        .map(|decimal| decimal.to_string())
        .map_err(|_| {
            JsonbDocError::new(
                "decimal.rejected",
                "/raw_bson_hex",
                "Decimal128 scale is outside the exact range",
            )
        })
}

fn format_uuid(bytes: &[u8; 16]) -> Result<String, JsonbDocError> {
    let version = bytes[6] >> 4;
    let variant = bytes[8] >> 6;
    if !(1..=8).contains(&version) || variant != 0b10 {
        return Err(JsonbDocError::new(
            "uuid.invalid",
            "/raw_bson_hex",
            "UUID subtype is not an RFC 4122 uuid",
        ));
    }
    Ok(format!(
        "{}-{}-{}-{}-{}",
        hex_encode(&bytes[0..4]),
        hex_encode(&bytes[4..6]),
        hex_encode(&bytes[6..8]),
        hex_encode(&bytes[8..10]),
        hex_encode(&bytes[10..16])
    ))
}

fn json_number(lexeme: &str) -> Result<Value, JsonbDocError> {
    serde_json::from_str(lexeme).map_err(|_| {
        JsonbDocError::new(
            "schema.invalid",
            "/raw_bson_hex",
            "integer lexeme is not a JSON number",
        )
    })
}

fn json_string_object(pairs: &[(&str, &str)]) -> Result<Value, JsonbDocError> {
    let mut object = Map::new();
    for (key, value) in pairs {
        object.insert((*key).to_string(), Value::String((*value).to_string()));
    }
    Ok(Value::Object(object))
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= data.len() {
        let chunk = (u32::from(data[index]) << 16)
            | (u32::from(data[index + 1]) << 8)
            | u32::from(data[index + 2]);
        out.push(TABLE[((chunk >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((chunk >> 12) & 0x3F) as usize] as char);
        out.push(TABLE[((chunk >> 6) & 0x3F) as usize] as char);
        out.push(TABLE[(chunk & 0x3F) as usize] as char);
        index += 3;
    }
    let rest = data.len() - index;
    if rest == 1 {
        let chunk = u32::from(data[index]) << 16;
        out.push(TABLE[((chunk >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((chunk >> 12) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rest == 2 {
        let chunk = (u32::from(data[index]) << 16) | (u32::from(data[index + 1]) << 8);
        out.push(TABLE[((chunk >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((chunk >> 12) & 0x3F) as usize] as char);
        out.push(TABLE[((chunk >> 6) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn read_i32(bytes: &[u8]) -> Result<i32, JsonbDocError> {
    if bytes.len() < 4 {
        return Err(truncated());
    }
    Ok(i32::from_le_bytes(copy4(bytes)))
}

fn copy4(bytes: &[u8]) -> [u8; 4] {
    [bytes[0], bytes[1], bytes[2], bytes[3]]
}

fn copy8(bytes: &[u8]) -> [u8; 8] {
    [
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]
}

fn truncated() -> JsonbDocError {
    JsonbDocError::new("pg.truncated", "/raw_bson_hex", "BSON value is truncated")
}

fn closed_dataset(value: &str) -> bool {
    let Some((schema, table)) = value.split_once('.') else {
        return false;
    };
    closed_ident(schema) && closed_ident(table)
}

fn closed_ident(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pg_pair(name: &str) -> (&'static [u8], &'static str) {
        match name {
            "empty_object" => (
                include_bytes!("../tests/fixtures/jsonb3802/empty_object.bin"),
                include_str!("../tests/fixtures/jsonb3802/empty_object.txt"),
            ),
            "obj_null" => (
                include_bytes!("../tests/fixtures/jsonb3802/obj_null.bin"),
                include_str!("../tests/fixtures/jsonb3802/obj_null.txt"),
            ),
            "obj_bools" => (
                include_bytes!("../tests/fixtures/jsonb3802/obj_bools.bin"),
                include_str!("../tests/fixtures/jsonb3802/obj_bools.txt"),
            ),
            "align_str_num" => (
                include_bytes!("../tests/fixtures/jsonb3802/align_str_num.bin"),
                include_str!("../tests/fixtures/jsonb3802/align_str_num.txt"),
            ),
            "scale_150" => (
                include_bytes!("../tests/fixtures/jsonb3802/scale_150.bin"),
                include_str!("../tests/fixtures/jsonb3802/scale_150.txt"),
            ),
            "scale_small" => (
                include_bytes!("../tests/fixtures/jsonb3802/scale_small.bin"),
                include_str!("../tests/fixtures/jsonb3802/scale_small.txt"),
            ),
            "num_10000" => (
                include_bytes!("../tests/fixtures/jsonb3802/num_10000.bin"),
                include_str!("../tests/fixtures/jsonb3802/num_10000.txt"),
            ),
            "key_order" => (
                include_bytes!("../tests/fixtures/jsonb3802/key_order.bin"),
                include_str!("../tests/fixtures/jsonb3802/key_order.txt"),
            ),
            "nested" => (
                include_bytes!("../tests/fixtures/jsonb3802/nested.bin"),
                include_str!("../tests/fixtures/jsonb3802/nested.txt"),
            ),
            "unicode" => (
                include_bytes!("../tests/fixtures/jsonb3802/unicode.bin"),
                include_str!("../tests/fixtures/jsonb3802/unicode.txt"),
            ),
            "big_numeric" => (
                include_bytes!("../tests/fixtures/jsonb3802/big_numeric.bin"),
                include_str!("../tests/fixtures/jsonb3802/big_numeric.txt"),
            ),
            "ext_long" => (
                include_bytes!("../tests/fixtures/jsonb3802/ext_long.bin"),
                include_str!("../tests/fixtures/jsonb3802/ext_long.txt"),
            ),
            "all_scalars" => (
                include_bytes!("../tests/fixtures/jsonb3802/all_scalars.bin"),
                include_str!("../tests/fixtures/jsonb3802/all_scalars.txt"),
            ),
            "scalar_null" => (
                include_bytes!("../tests/fixtures/jsonb3802/scalar_null.bin"),
                include_str!("../tests/fixtures/jsonb3802/scalar_null.txt"),
            ),
            "array_mixed" => (
                include_bytes!("../tests/fixtures/jsonb3802/array_mixed.bin"),
                include_str!("../tests/fixtures/jsonb3802/array_mixed.txt"),
            ),
            "quote" => (
                include_bytes!("../tests/fixtures/jsonb3802/quote.bin"),
                include_str!("../tests/fixtures/jsonb3802/quote.txt"),
            ),
            _ => panic!("unknown fixture {name}"),
        }
    }

    #[test]
    fn postgres_jsonb_send_files_keep_exact_text() {
        for name in [
            "empty_object",
            "obj_null",
            "obj_bools",
            "align_str_num",
            "scale_150",
            "scale_small",
            "num_10000",
            "key_order",
            "nested",
            "unicode",
            "big_numeric",
            "ext_long",
            "all_scalars",
            "scalar_null",
            "array_mixed",
            "quote",
        ] {
            let (bytes, text) = pg_pair(name);
            let file = inspect_jsonb_send(bytes).unwrap_or_else(|err| panic!("{name}: {err}"));
            assert_eq!(file.version, 1, "{name}");
            assert_eq!(file.text, text, "{name}");
            assert_eq!(&bytes[1..], text.as_bytes(), "{name}");
        }
        let scale = inspect_jsonb_send(pg_pair("scale_150").0).expect("scale");
        let lexeme = scale.value["n"].as_number().expect("n").to_string();
        assert_eq!(lexeme, "1.50");
        let parsed = ExactDecimal::parse_sql(&lexeme).expect("exact");
        assert_eq!(parsed.scale(), 2);
        assert_eq!(parsed.coefficient(), "150");
        let small = inspect_jsonb_send(pg_pair("scale_small").0).expect("small");
        assert_eq!(small.value["n"].as_number().expect("n").to_string(), "-0.001");
    }

    #[test]
    fn json_send_bad_version_and_truncation_are_refused() {
        let json_send = include_bytes!("../tests/fixtures/jsonb3802/json_send_a1.bin");
        let err = inspect_jsonb_send(json_send).expect_err("json send");
        assert!(err.code.ends_with("/json_text_not_jsonb"));
        let original = pg_pair("empty_object").0.to_vec();
        let err = inspect_jsonb_send(&original[..1]).expect_err("version byte only");
        assert!(err.code.ends_with("/pg.truncated"), "{}", err.code);
        let mut bad = original;
        bad[0] = 2;
        let err = inspect_jsonb_send(&bad).expect_err("version");
        assert!(err.code.ends_with("/pg.version"), "{}", err.code);
        let err = inspect_jsonb_send(&bad[..1]).expect_err("lone bad version");
        assert!(err.code.ends_with("/pg.version"), "{}", err.code);
        let err = inspect_jsonb_send(&[1, b'{']).expect_err("cut");
        assert!(err.code.ends_with("/pg.truncated"), "{}", err.code);
        let err = inspect_jsonb_send(&[1, b'1', b'e', b'2']).expect_err("exponent");
        assert!(err.code.ends_with("/float.rejected"));
    }

    #[test]
    fn decimal128_known_vector_keeps_scale_and_rejects_non_finite() {
        let one = decimal_bytes(false, 1, 0);
        assert_eq!(u64::from_le_bytes(copy8(&one)), 1);
        assert_eq!(u64::from_le_bytes(copy8(&one[8..])), 0x3040_0000_0000_0000);
        assert_eq!(format_decimal128(&one).expect("1"), "1");
        let zero = decimal_bytes(false, 0, 0);
        assert_eq!(u64::from_le_bytes(copy8(&zero[8..])), 0x3040_0000_0000_0000);
        assert_eq!(format_decimal128(&zero).expect("0"), "0");
        let neg = decimal_bytes(true, 1, 0);
        assert_eq!(u64::from_le_bytes(copy8(&neg[8..])), 0xB040_0000_0000_0000);
        assert_eq!(format_decimal128(&neg).expect("-1"), "-1");
        let scaled = decimal_bytes(false, 12300, -4);
        assert_eq!(format_decimal128(&scaled).expect("scale"), "1.2300");
        let neg_scale = decimal_bytes(true, 150, -2);
        assert_eq!(format_decimal128(&neg_scale).expect("-1.50"), "-1.50");
        let mut nan = [0_u8; 16];
        nan[8..].copy_from_slice(&(0x1_F000_u64 << 46).to_le_bytes());
        assert!(format_decimal128(&nan).unwrap_err().code.ends_with("/decimal.rejected"));
        let mut infinity = [0_u8; 16];
        infinity[8..].copy_from_slice(&(0x1_E000_u64 << 46).to_le_bytes());
        assert!(format_decimal128(&infinity)
            .unwrap_err()
            .code
            .ends_with("/decimal.rejected"));
    }

    #[test]
    fn empty_document_seals_against_the_postgres_file() {
        let raw = encode_document(&[]);
        assert_eq!(raw, vec![5, 0, 0, 0, 0]);
        let (bytes, text) = pg_pair("empty_object");
        let sealed = seal_bson_jsonb_document(&envelope("public.docs", "doc", &raw, bytes, text, false))
            .expect("seal");
        assert_eq!(sealed.oid, 3802);
        assert_eq!(sealed.typmod, -1);
        assert_eq!(sealed.arrow_type, "utf8");
        assert_eq!(sealed.metadata.get("oo.storage.type").map(String::as_str), Some("jsonb"));
        assert_eq!(sealed.metadata.get("oo.storage.oid").map(String::as_str), Some("3802"));
        assert_eq!(sealed.metadata.get("oo.document.missing").map(String::as_str), Some("absent_not_null"));
        assert_eq!(sealed.metadata.get("oo.precision.float").map(String::as_str), Some(FLOAT_PRECISION));
        assert_eq!(sealed.metadata.get("oo.document.double").map(String::as_str), Some(DOUBLE_ENCODING));
        assert_eq!(
            sealed.metadata.get("oo.document.double.negative_zero").map(String::as_str),
            Some(DOUBLE_NEGATIVE_ZERO)
        );
        assert_eq!(
            sealed.metadata.get("oo.document.double.non_finite").map(String::as_str),
            Some(DOUBLE_NON_FINITE)
        );
        assert_eq!(
            sealed.metadata.get("oo.document.double.nan_payload").map(String::as_str),
            Some(DOUBLE_NAN_PAYLOAD)
        );
        assert_eq!(
            sealed.metadata.get("oo.document.field_scope").map(String::as_str),
            Some(FIELD_SCOPE_UNCHECKED)
        );
        assert_eq!(sealed.metadata.get("oo.quality.loss_concealed").map(String::as_str), Some("false"));
        assert_eq!(sealed.extended_json, serde_json::json!({}));
        assert!(sealed.extended_json.get("gone").is_none());
    }

    #[test]
    fn typed_document_matches_postgres_and_keeps_exact_values() {
        let raw = canonical_bson();
        let bytes = include_bytes!("../tests/fixtures/jsonb3802/canonical.bin");
        let text = include_str!("../tests/fixtures/jsonb3802/canonical.txt");
        assert_eq!(
            raw.as_slice(),
            include_bytes!("../tests/fixtures/jsonb3802/canonical-bson.bin").as_slice()
        );
        let sealed = seal_bson_jsonb_document(&envelope("public.docs", "doc", &raw, bytes, text, true))
            .expect("canonical");
        let expected: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/jsonb3802/canonical-extended.json"
        ))
        .expect("extended");
        assert!(values_eq(&sealed.extended_json, &expected));
        assert!(sealed.extended_json.get("gone").is_none());
        assert!(sealed.extended_json.get("note").unwrap().is_null());
        assert_eq!(
            sealed.extended_json["big"]["$numberLong"].as_str(),
            Some("9223372036854775807")
        );
        assert_eq!(sealed.extended_json["fits"]["$numberLong"].as_str(), Some("5"));
        assert_eq!(sealed.extended_json["n"].as_number().unwrap().to_string(), "7");
        assert_eq!(sealed.extended_json["d"]["$numberDecimal"].as_str(), Some("1.2300"));
        assert_eq!(sealed.extended_json["negd"]["$numberDecimal"].as_str(), Some("-1.50"));
        assert_eq!(
            sealed.extended_json["u"]["$uuid"].as_str(),
            Some("00112233-4455-4677-8899-aabbccddeeff")
        );
        assert_eq!(
            sealed.extended_json["when"]["$date"]["$numberLong"].as_str(),
            Some("-61000")
        );
        assert_eq!(
            sealed.extended_json["b"]["$binary"]["subType"].as_str(),
            Some("00")
        );
        assert_eq!(sealed.raw_bson_sha256, payload_sha256(&raw));
        let json_send = include_bytes!("../tests/fixtures/jsonb3802/canonical-json-send.bin");
        let err = seal_bson_jsonb_document(&envelope(
            "public.docs",
            "doc",
            &raw,
            json_send,
            text,
            true,
        ))
        .expect_err("json send");
        assert!(err.code.ends_with("/json_text_not_jsonb"));
    }

    #[test]
    fn wrong_oid_typmod_dropped_bson_and_lossy_flags_are_refused() {
        let raw = encode_document(&[]);
        let (bytes, text) = pg_pair("empty_object");
        let mut input: Value = serde_json::from_str(&envelope(
            "public.docs",
            "doc",
            &raw,
            bytes,
            text,
            false,
        ))
        .unwrap();
        input["oid"] = json_number("114").unwrap();
        let err = seal_bson_jsonb_document(&input.to_string()).expect_err("114");
        assert!(err.code.ends_with("/json114_not_jsonb"));
        input["oid"] = json_number("3802").unwrap();
        input["typmod"] = json_number("0").unwrap();
        let err = seal_bson_jsonb_document(&input.to_string()).expect_err("typmod");
        assert!(err.code.ends_with("/typmod.rejected"));
        input["typmod"] = json_number("-1").unwrap();
        input["source_type"] = Value::String("json".to_string());
        let err = seal_bson_jsonb_document(&input.to_string()).expect_err("json token");
        assert!(err.code.ends_with("/source_type.rejected"));
        input["source_type"] = Value::String("jsonb".to_string());
        input["raw_bson_dropped"] = Value::Bool(true);
        let err = seal_bson_jsonb_document(&input.to_string()).expect_err("dropped");
        assert!(err.code.ends_with("/raw_bson_dropped"));
        input["raw_bson_dropped"] = Value::Bool(false);
        input["raw_bson_hex"] = Value::String(String::new());
        let err = seal_bson_jsonb_document(&input.to_string()).expect_err("empty");
        assert!(err.code.ends_with("/raw_bson_dropped"));
        input["raw_bson_hex"] = Value::String(hex_encode(&raw));
        input["raw_bson_sha256"] = Value::String("ab".repeat(32));
        let err = seal_bson_jsonb_document(&input.to_string()).expect_err("hash");
        assert!(err.code.ends_with("/digest.mismatch"));
        input["raw_bson_sha256"] = Value::Null;
        input["loss_concealed"] = Value::Bool(true);
        let err = seal_bson_jsonb_document(&input.to_string()).expect_err("loss");
        assert!(err.code.ends_with("/loss_concealed"));
        input["loss_concealed"] = Value::Bool(false);
        input["missing_is_null"] = Value::Bool(true);
        let err = seal_bson_jsonb_document(&input.to_string()).expect_err("missing");
        assert!(err.code.ends_with("/missing_is_null"));
        let array = pg_pair("scalar_null");
        let err = seal_bson_jsonb_document(&envelope(
            "public.docs",
            "doc",
            &raw,
            array.0,
            array.1,
            false,
        ))
        .expect_err("scalar");
        assert!(err.code.ends_with("/pg.mismatch"));
    }

    #[test]
    fn double_keeps_bits_and_does_not_render_int64_or_decimal() {
        let samples = [
            0.0f64.to_bits(),
            (-0.0f64).to_bits(),
            1.0f64.to_bits(),
            (-1.5f64).to_bits(),
            41.25f64.to_bits(),
            0.1f64.to_bits(),
            f64::MIN.to_bits(),
            f64::MAX.to_bits(),
            f64::MIN_POSITIVE.to_bits(),
            1e21_f64.to_bits(),
            f64::from_bits(1).to_bits(),
        ];
        for bits in samples {
            let text = canonical_number_double(bits);
            let back: f64 = text.parse().expect("parse");
            assert_eq!(back.to_bits(), bits, "{text}");
        }
        assert_eq!(canonical_number_double(f64::NAN.to_bits()), "NaN");
        assert_eq!(canonical_number_double(0x7ff0_0000_0000_0001), "NaN");
        assert_eq!(canonical_number_double(0xfff8_0000_0000_0001), "NaN");
        assert_eq!(canonical_number_double(f64::INFINITY.to_bits()), "Infinity");
        assert_eq!(canonical_number_double(f64::NEG_INFINITY.to_bits()), "-Infinity");
        assert_eq!(canonical_number_double((-0.0f64).to_bits()), "-0.0");
        assert_eq!(canonical_number_double(0.0f64.to_bits()), "0.0");
        assert_eq!(canonical_number_double(41.25f64.to_bits()), "41.25");

        let raw = encode_document(&[
            ("big", Bson::Int64(9_223_372_036_854_775_807)),
            ("d", Bson::Decimal128(decimal_bytes(false, 12300, -4))),
            ("gone_absent", Bson::Int32(1)),
            ("neg0", Bson::Double((-0.0f64).to_bits())),
            ("note", Bson::Null),
            ("weight", Bson::Double(41.25f64.to_bits())),
            ("wide", Bson::Double(f64::INFINITY.to_bits())),
        ]);
        let value = decode_bson_document(&raw).expect("mixed");
        assert!(value.get("missing").is_none());
        assert!(value.get("note").unwrap().is_null());
        assert_eq!(value["big"]["$numberLong"], "9223372036854775807");
        assert_eq!(value["d"]["$numberDecimal"], "1.2300");
        assert_eq!(value["weight"]["$numberDouble"], "41.25");
        assert_eq!(value["neg0"]["$numberDouble"], "-0.0");
        assert_eq!(value["wide"]["$numberDouble"], "Infinity");
        let mut short = encode_document(&[("n", Bson::Int32(1))]);
        short[4] = 0x01;
        let err = decode_bson_document(&short).expect_err("short double");
        assert!(err.code.ends_with("/pg.truncated"), "{}", err.code);
    }

    #[test]
    fn approved_scope_refuses_an_extra_root_key_and_keeps_a_missing_key() {
        let raw = encode_document(&[("weight", Bson::Double(41.25f64.to_bits()))]);
        let text = r#"{"weight": {"$numberDouble": "41.25"}}"#;
        let pg = send_file(text);
        let mut input: Value = serde_json::from_str(&envelope(
            "public.docs",
            "doc",
            &raw,
            &pg,
            text,
            false,
        ))
        .unwrap();
        input["approved_fields"] = serde_json::json!(["_id", "code", "weight", "whCode"]);
        let sealed = seal_bson_jsonb_document(&input.to_string()).expect("in scope");
        assert!(sealed.extended_json.get("_id").is_none());
        assert!(sealed.extended_json.get("code").is_none());
        assert_eq!(sealed.extended_json["weight"]["$numberDouble"], "41.25");
        assert_eq!(
            sealed.metadata.get("oo.document.field_scope").map(String::as_str),
            Some(FIELD_SCOPE_APPROVED)
        );
        let extra = encode_document(&[
            ("notApproved", Bson::Int32(1)),
            ("weight", Bson::Double(41.25f64.to_bits())),
        ]);
        input["raw_bson_hex"] = Value::String(hex_encode(&extra));
        let err = seal_bson_jsonb_document(&input.to_string()).expect_err("scope");
        assert!(err.code.ends_with("/field.scope"), "{}", err.code);
        input["raw_bson_hex"] = Value::String(hex_encode(&raw));
        input["approved_fields"] = serde_json::json!(["weight", "weight"]);
        let err = seal_bson_jsonb_document(&input.to_string()).expect_err("dup names");
        assert!(err.code.ends_with("/schema.invalid"), "{}", err.code);
        input["approved_fields"] = serde_json::json!("weight");
        let err = seal_bson_jsonb_document(&input.to_string()).expect_err("not array");
        assert!(err.code.ends_with("/schema.invalid"), "{}", err.code);
    }

    fn send_file(text: &str) -> Vec<u8> {
        let mut bytes = vec![JSONB_SEND_VERSION];
        bytes.extend(text.as_bytes());
        bytes
    }

    #[test]
    fn duplicate_unsupported_and_bad_uuid_are_refused() {
        let dup = encode_document(&[("a", Bson::Null), ("a", Bson::Bool(true))]);
        let err = decode_bson_document(&dup).expect_err("dup");
        assert!(err.code.ends_with("/duplicate_key"));
        let mut stamp = encode_document(&[("t", Bson::Null)]);
        stamp[4] = 0x11;
        let err = decode_bson_document(&stamp).expect_err("timestamp");
        assert!(err.code.ends_with("/type.unsupported"));
        let bad_uuid = encode_document(&[(
            "u",
            Bson::Binary {
                subtype: 4,
                data: vec![0; 16],
            },
        )]);
        let err = decode_bson_document(&bad_uuid).expect_err("uuid");
        assert!(err.code.ends_with("/uuid.invalid"));
        let short_uuid = encode_document(&[(
            "u",
            Bson::Binary {
                subtype: 4,
                data: vec![1, 2, 3],
            },
        )]);
        let err = decode_bson_document(&short_uuid).expect_err("short uuid");
        assert!(err.code.ends_with("/uuid.invalid"));
    }

    fn envelope(
        dataset: &str,
        column: &str,
        raw: &[u8],
        pg: &[u8],
        text: &str,
        claim_hash: bool,
    ) -> String {
        let mut input = serde_json::json!({
            "protocol": PROTOCOL,
            "dataset": dataset,
            "column": column,
            "oid": JSONB_OID,
            "typmod": JSONB_TYPMOD,
            "source_type": "jsonb",
            "raw_bson_dropped": false,
            "raw_bson_hex": hex_encode(raw),
            "pg_jsonb_hex": hex_encode(pg),
            "pg_jsonb_text": text,
            "loss_concealed": false,
            "missing_is_null": false
        });
        if claim_hash {
            input["raw_bson_sha256"] = Value::String(payload_sha256(raw));
        }
        input.to_string()
    }

    fn decimal_bytes(negative: bool, coeff: u128, exp: i32) -> [u8; 16] {
        assert!(coeff < COEFF_LIMIT);
        let biased = u64::try_from(exp + DECIMAL_BIAS).unwrap();
        let msd = coeff >> 110;
        let trailing = coeff & ((1_u128 << 110) - 1);
        assert!(msd <= 9);
        let combination = if msd < 8 {
            (biased << 3) | (msd as u64)
        } else {
            0x1_8000 | (biased << 1) | ((msd as u64) & 1)
        };
        let mut high = (combination << 46) | ((trailing >> 64) as u64);
        if negative {
            high |= 1_u64 << 63;
        }
        let low = trailing as u64;
        let mut out = [0_u8; 16];
        out[..8].copy_from_slice(&low.to_le_bytes());
        out[8..].copy_from_slice(&high.to_le_bytes());
        out
    }

    enum Bson {
        String(String),
        Document(Vec<(&'static str, Bson)>),
        Array(Vec<Bson>),
        Binary { subtype: u8, data: Vec<u8> },
        ObjectId([u8; 12]),
        Bool(bool),
        DateTime(i64),
        Null,
        Int32(i32),
        Int64(i64),
        Double(u64),
        Decimal128([u8; 16]),
    }

    fn canonical_bson() -> Vec<u8> {
        let oid = hex_bytes("507f1f77bcf86cd799439011");
        let mut object_id = [0_u8; 12];
        object_id.copy_from_slice(&oid);
        let uuid = hex_bytes("00112233445546778899aabbccddeeff");
        encode_document(&[
            ("_id", Bson::ObjectId(object_id)),
            (
                "b",
                Bson::Binary {
                    subtype: 0,
                    data: vec![0],
                },
            ),
            ("big", Bson::Int64(9_223_372_036_854_775_807)),
            ("d", Bson::Decimal128(decimal_bytes(false, 12300, -4))),
            ("fits", Bson::Int64(5)),
            ("flag", Bson::Bool(false)),
            ("n", Bson::Int32(7)),
            ("neg", Bson::Int32(-2)),
            ("negd", Bson::Decimal128(decimal_bytes(true, 150, -2))),
            (
                "nested",
                Bson::Document(vec![(
                    "a",
                    Bson::Array(vec![Bson::Int32(1), Bson::String("x".to_string())]),
                )]),
            ),
            ("note", Bson::Null),
            ("ok", Bson::Bool(true)),
            ("title", Bson::String("雪".to_string())),
            (
                "u",
                Bson::Binary {
                    subtype: 4,
                    data: uuid,
                },
            ),
            ("when", Bson::DateTime(-61_000)),
        ])
    }

    fn encode_document(fields: &[(&str, Bson)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (key, value) in fields {
            body.push(type_byte(value));
            body.extend(key.as_bytes());
            body.push(0);
            encode_value(&mut body, value);
        }
        body.push(0);
        let mut out = ((body.len() + 4) as i32).to_le_bytes().to_vec();
        out.extend(body);
        out
    }

    fn type_byte(value: &Bson) -> u8 {
        match value {
            Bson::String(_) => 0x02,
            Bson::Document(_) => 0x03,
            Bson::Array(_) => 0x04,
            Bson::Binary { .. } => 0x05,
            Bson::ObjectId(_) => 0x07,
            Bson::Bool(_) => 0x08,
            Bson::DateTime(_) => 0x09,
            Bson::Null => 0x0A,
            Bson::Int32(_) => 0x10,
            Bson::Int64(_) => 0x12,
            Bson::Double(_) => 0x01,
            Bson::Decimal128(_) => 0x13,
        }
    }

    fn encode_value(out: &mut Vec<u8>, value: &Bson) {
        match value {
            Bson::String(text) => {
                let len = i32::try_from(text.len() + 1).unwrap();
                out.extend(len.to_le_bytes());
                out.extend(text.as_bytes());
                out.push(0);
            }
            Bson::Document(fields) => out.extend(encode_document(fields)),
            Bson::Array(items) => {
                let keys: Vec<String> = (0..items.len()).map(|index| index.to_string()).collect();
                let fields = items
                    .iter()
                    .zip(keys.iter())
                    .map(|(item, key)| (key.as_str(), item.clone_ref()))
                    .collect::<Vec<_>>();
                out.extend(encode_document(&fields));
            }
            Bson::Binary { subtype, data } => {
                out.extend((data.len() as i32).to_le_bytes());
                out.push(*subtype);
                out.extend(data);
            }
            Bson::ObjectId(bytes) => out.extend(bytes),
            Bson::Bool(value) => out.push(u8::from(*value)),
            Bson::DateTime(millis) => out.extend(millis.to_le_bytes()),
            Bson::Null => {}
            Bson::Int32(value) => out.extend(value.to_le_bytes()),
            Bson::Int64(value) => out.extend(value.to_le_bytes()),
            Bson::Double(bits) => out.extend(bits.to_le_bytes()),
            Bson::Decimal128(bytes) => out.extend(bytes),
        }
    }

    impl Bson {
        fn clone_ref(&self) -> Bson {
            match self {
                Self::String(text) => Self::String(text.clone()),
                Self::Document(fields) => Self::Document(
                    fields
                        .iter()
                        .map(|(key, value)| (*key, value.clone_ref()))
                        .collect(),
                ),
                Self::Array(items) => Self::Array(items.iter().map(Self::clone_ref).collect()),
                Self::Binary { subtype, data } => Self::Binary {
                    subtype: *subtype,
                    data: data.clone(),
                },
                Self::ObjectId(bytes) => Self::ObjectId(*bytes),
                Self::Bool(value) => Self::Bool(*value),
                Self::DateTime(value) => Self::DateTime(*value),
                Self::Null => Self::Null,
                Self::Int32(value) => Self::Int32(*value),
                Self::Int64(value) => Self::Int64(*value),
                Self::Double(bits) => Self::Double(*bits),
                Self::Decimal128(bytes) => Self::Decimal128(*bytes),
            }
        }
    }

    fn hex_bytes(text: &str) -> Vec<u8> {
        decode_hex(text, "/test").expect("hex")
    }

    const W141_RAW_SHA: &str = "4be38f2f45bcbccdcd062861d472bdc859dc0a54bd1e38258d970266c2dcfdd3";
    const W141_APPROVED: &[&str] = &[
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

    /// Customer emit and seal. Idle unless `OO_BSON_EMIT_DIR` is set, so a
    /// normal `cargo test` does not claim the fixed shipment row was proved.
    #[test]
    fn w141_bson_double_probe_is_idle_without_emit_dir() {
        let Ok(dir) = std::env::var("OO_BSON_EMIT_DIR") else {
            return;
        };
        match std::env::var("OO_BSON_PHASE").unwrap_or_else(|_| "emit".to_string()).as_str() {
            "emit" => w141_emit(&dir),
            "seal" => w141_seal(&dir),
            other => panic!("OO_BSON_PHASE {other}"),
        }
    }

    fn w141_emit(dir: &str) {
        let root = std::path::Path::new(dir);
        let emit = root.join("emit");
        std::fs::create_dir_all(&emit).expect("emit dir");
        let document_path = std::env::var("OO_W141_DOCUMENT").expect("OO_W141_DOCUMENT");
        let fields_path = std::env::var("OO_W141_FIELDS").expect("OO_W141_FIELDS");
        let document: Value = serde_json::from_str(&std::fs::read_to_string(&document_path).expect("document")).expect("document json");
        let fields: Value = serde_json::from_str(&std::fs::read_to_string(&fields_path).expect("fields")).expect("fields json");
        let approved = fields["approved_fields"].as_array().expect("approved_fields");
        let approved_names: Vec<&str> = approved.iter().filter_map(Value::as_str).collect();
        assert_eq!(approved_names, W141_APPROVED);
        let raw_hex = document["raw_bson_hex"].as_str().expect("raw hex");
        let raw = decode_hex(raw_hex, "/raw_bson_hex").expect("raw");
        assert_eq!(payload_sha256(&raw), W141_RAW_SHA);
        assert_eq!(document["raw_bson_sha256"].as_str(), Some(W141_RAW_SHA));
        assert_eq!(document["dataset"].as_str(), Some("oo_w141.shipments"));
        assert_eq!(document["column"].as_str(), Some("doc"));
        let original = bson_to_extended_json(&raw).expect("original");
        let original_keys = object_keys(&original);
        assert!(!original_keys.iter().any(|key| key == "weight"));
        assert!(original_keys.iter().all(|key| W141_APPROVED.contains(&key.as_str())));
        let bits = 41.25_f64.to_bits();
        assert_eq!(canonical_number_double(bits), "41.25");
        assert_eq!("41.25".parse::<f64>().unwrap().to_bits(), bits);
        let completed = append_root_field(&raw, 0x01, "weight", &bits.to_le_bytes());
        assert_eq!(&completed[4..4 + (raw.len() - 5)], &raw[4..raw.len() - 1]);
        let value = bson_to_extended_json(&completed).expect("completed");
        assert_eq!(value["weight"]["$numberDouble"], "41.25");
        for key in &original_keys {
            assert!(values_eq(&value[key], &original[key]), "{key}");
        }
        write_json(&emit.join("policy.json"), &serde_json::json!({
            "double": DOUBLE_ENCODING,
            "negative_zero": DOUBLE_NEGATIVE_ZERO,
            "non_finite": DOUBLE_NON_FINITE,
            "nan_payload": DOUBLE_NAN_PAYLOAD,
            "float": FLOAT_PRECISION,
            "field_scope_when_listed": FIELD_SCOPE_APPROVED,
            "weight_bits_hex": hex_encode(&bits.to_le_bytes()),
            "weight_canonical": "41.25",
            "original_sha256": W141_RAW_SHA,
            "completed_sha256": payload_sha256(&completed),
            "original_root_keys": original_keys,
        }));
        write_case(&emit, "original", &raw);
        write_case(&emit, "customer", &completed);
        write_case(&emit, "neg0", &encode_document(&[("z", Bson::Double((-0.0_f64).to_bits()))]));
        write_case(&emit, "nan", &encode_document(&[("z", Bson::Double(0x7ff0_0000_0000_0001))]));
        write_case(&emit, "posinf", &encode_document(&[("z", Bson::Double(f64::INFINITY.to_bits()))]));
        write_case(&emit, "neginf", &encode_document(&[("z", Bson::Double(f64::NEG_INFINITY.to_bits()))]));
        write_case(
            &emit,
            "exact",
            &encode_document(&[
                ("big", Bson::Int64(9_223_372_036_854_775_807)),
                ("d", Bson::Decimal128(decimal_bytes(false, 12300, -4))),
                ("neg0", Bson::Double((-0.0_f64).to_bits())),
                ("nested", Bson::Document(vec![("a", Bson::Array(vec![Bson::Int32(1), Bson::String("x".to_string())]))])),
                ("note", Bson::Null),
                ("weight", Bson::Double(bits)),
            ]),
        );
        let scope = append_root_field(&completed, 0x10, "notApproved", &1_i32.to_le_bytes());
        write_case(&emit, "scope", &scope);
        let mut refusals = Vec::new();
        let mut stamp = encode_document(&[("t", Bson::Null)]);
        stamp[4] = 0x11;
        match bson_to_extended_json(&stamp) {
            Ok(_) => panic!("timestamp must be refused"),
            Err(err) => refusals.push(refusal("timestamp", &err)),
        }
        let mut short = encode_document(&[("n", Bson::Int32(1))]);
        short[4] = 0x01;
        match bson_to_extended_json(&short) {
            Ok(_) => panic!("short double must be refused"),
            Err(err) => refusals.push(refusal("short_double", &err)),
        }
        write_json(&emit.join("decode-refusals.json"), &Value::Array(refusals));
    }

    fn w141_seal(dir: &str) {
        let root = std::path::Path::new(dir);
        let seal_dir = root.join("seal");
        std::fs::create_dir_all(&seal_dir).expect("seal dir");
        let cases = ["original", "customer", "neg0", "nan", "posinf", "neginf", "exact", "scope"];
        let mut summary = Vec::new();
        for name in cases {
            let raw = hex_bytes(&read_trimmed(&root.join("emit").join(format!("{name}.raw.hex"))));
            let pg = hex_bytes(&read_trimmed(&root.join("pg").join(format!("{name}.hex"))));
            let text = read_trimmed(&root.join("pg").join(format!("{name}.text")));
            let mut input: Value = serde_json::from_str(&envelope(
                "oo_w141.shipments",
                "doc",
                &raw,
                &pg,
                &text,
                true,
            ))
            .unwrap();
            if matches!(name, "original" | "customer" | "scope") {
                input["approved_fields"] = serde_json::json!(W141_APPROVED);
            }
            match seal_bson_jsonb_document(&input.to_string()) {
                Ok(sealed) => {
                    if name == "scope" {
                        panic!("notApproved sealed");
                    }
                    let body = serde_json::json!({
                        "name": name,
                        "ok": true,
                        "field_scope": sealed.metadata.get("oo.document.field_scope"),
                        "double": sealed.metadata.get("oo.document.double"),
                        "negative_zero": sealed.metadata.get("oo.document.double.negative_zero"),
                        "non_finite": sealed.metadata.get("oo.document.double.non_finite"),
                        "nan_payload": sealed.metadata.get("oo.document.double.nan_payload"),
                        "float": sealed.metadata.get("oo.precision.float"),
                        "raw_bson_sha256": sealed.raw_bson_sha256,
                        "pg_jsonb_sha256": sealed.pg_jsonb_sha256,
                        "extended_json": sealed.extended_json,
                    });
                    write_json(&seal_dir.join(format!("{name}.json")), &body);
                    match name {
                        "customer" => {
                            assert_eq!(sealed.extended_json["weight"]["$numberDouble"], "41.25");
                            assert!(sealed.extended_json.get("blob_presence").is_none());
                            assert_eq!(
                                sealed.metadata.get("oo.document.field_scope").map(String::as_str),
                                Some(FIELD_SCOPE_APPROVED)
                            );
                        }
                        "original" => assert!(sealed.extended_json.get("weight").is_none()),
                        "neg0" => assert_eq!(sealed.extended_json["z"]["$numberDouble"], "-0.0"),
                        "nan" => assert_eq!(sealed.extended_json["z"]["$numberDouble"], "NaN"),
                        "posinf" => assert_eq!(sealed.extended_json["z"]["$numberDouble"], "Infinity"),
                        "neginf" => assert_eq!(sealed.extended_json["z"]["$numberDouble"], "-Infinity"),
                        "exact" => {
                            assert_eq!(sealed.extended_json["big"]["$numberLong"], "9223372036854775807");
                            assert_eq!(sealed.extended_json["d"]["$numberDecimal"], "1.2300");
                            assert!(sealed.extended_json.get("note").unwrap().is_null());
                            assert!(sealed.extended_json.get("missing").is_none());
                            assert_eq!(sealed.extended_json["weight"]["$numberDouble"], "41.25");
                            assert_eq!(sealed.extended_json["neg0"]["$numberDouble"], "-0.0");
                        }
                        _ => {}
                    }
                    if name == "customer" {
                        write_json(&seal_dir.join("customer.envelope.json"), &input);
                        let command_path = std::env::var("OO_W141_COMMAND").expect("OO_W141_COMMAND");
                        let mut command: Value = serde_json::from_str(&std::fs::read_to_string(command_path).expect("command")).expect("command");
                        command["document"] = input;
                        write_json(&seal_dir.join("command.json"), &command);
                        let original_raw = hex_bytes(&read_trimmed(&root.join("emit/original.raw.hex")));
                        let original_pg = hex_bytes(&read_trimmed(&root.join("pg/original.hex")));
                        let original_text = read_trimmed(&root.join("pg/original.text"));
                        let mut original_input: Value = serde_json::from_str(&envelope(
                            "oo_w141.shipments",
                            "doc",
                            &original_raw,
                            &original_pg,
                            &original_text,
                            true,
                        ))
                        .unwrap();
                        original_input["approved_fields"] = serde_json::json!(W141_APPROVED);
                        command["document"] = original_input;
                        write_json(&seal_dir.join("command-original.json"), &command);
                    }
                    summary.push(body);
                }
                Err(err) => {
                    if name != "scope" {
                        panic!("{name} seal failed: {err}");
                    }
                    assert!(err.code.ends_with("/field.scope"), "{}", err.code);
                    let body = serde_json::json!({
                        "name": name,
                        "ok": false,
                        "code": err.code,
                        "path": err.path,
                        "message": err.message,
                    });
                    write_json(&seal_dir.join("scope.json"), &body);
                    summary.push(body);
                }
            }
        }
        let customer_raw = hex_bytes(&read_trimmed(&root.join("emit/customer.raw.hex")));
        let customer_text = read_trimmed(&root.join("pg/customer.text"));
        let json_send = hex_bytes(&read_trimmed(&root.join("pg/customer.json_send.hex")));
        summary.push(expect_refuse(
            "json_send",
            &envelope("oo_w141.shipments", "doc", &customer_raw, &json_send, &customer_text, true),
            "/json_text_not_jsonb",
        ));
        let mut oid = serde_json::from_str::<Value>(&envelope(
            "oo_w141.shipments",
            "doc",
            &customer_raw,
            &hex_bytes(&read_trimmed(&root.join("pg/customer.hex"))),
            &customer_text,
            true,
        ))
        .unwrap();
        oid["oid"] = serde_json::json!(114);
        summary.push(expect_refuse("oid_114", &oid.to_string(), "/json114_not_jsonb"));
        oid["oid"] = serde_json::json!(3802);
        oid["typmod"] = serde_json::json!(0);
        summary.push(expect_refuse("typmod", &oid.to_string(), "/typmod.rejected"));
        let synthetic = r#"{"n":1e2}"#;
        let synthetic_pg = send_file(synthetic);
        summary.push(expect_refuse(
            "synthetic_exponent",
            &envelope("oo_w141.shipments", "doc", &customer_raw, &synthetic_pg, synthetic, true),
            "/float.rejected",
        ));
        write_json(&seal_dir.join("summary.json"), &Value::Array(summary));
    }

    fn expect_refuse(name: &str, input: &str, suffix: &str) -> Value {
        match seal_bson_jsonb_document(input) {
            Ok(_) => panic!("{name} sealed"),
            Err(err) => {
                assert!(err.code.ends_with(suffix), "{name} {}", err.code);
                serde_json::json!({
                    "name": name,
                    "ok": false,
                    "code": err.code,
                    "path": err.path,
                    "message": err.message,
                    "synthetic_send": name == "synthetic_exponent",
                })
            }
        }
    }

    fn write_case(dir: &std::path::Path, name: &str, raw: &[u8]) {
        let value = bson_to_extended_json(raw).unwrap_or_else(|err| panic!("{name}: {err}"));
        std::fs::write(dir.join(format!("{name}.raw.hex")), hex_encode(raw)).expect("raw");
        std::fs::write(dir.join(format!("{name}.extended.json")), serde_json::to_string(&value).expect("json")).expect("extended");
    }

    fn write_json(path: &std::path::Path, value: &Value) {
        std::fs::write(path, serde_json::to_string_pretty(value).expect("json")).expect("write");
    }

    fn object_keys(value: &Value) -> Vec<String> {
        let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        keys
    }

    fn refusal(name: &str, err: &JsonbDocError) -> Value {
        serde_json::json!({
            "name": name,
            "ok": false,
            "code": err.code,
            "path": err.path,
            "message": err.message,
        })
    }

    fn read_trimmed(path: &std::path::Path) -> String {
        let mut text = std::fs::read_to_string(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        while text.ends_with('\n') || text.ends_with('\r') {
            text.pop();
        }
        text
    }

    fn append_root_field(raw: &[u8], type_byte: u8, key: &str, payload: &[u8]) -> Vec<u8> {
        assert!(raw.len() >= 5, "document");
        assert_eq!(*raw.last().expect("tail"), 0);
        let declared = i32::from_le_bytes(copy4(&raw[..4]));
        assert_eq!(declared as usize, raw.len());
        let mut body = raw[4..raw.len() - 1].to_vec();
        body.push(type_byte);
        body.extend(key.as_bytes());
        body.push(0);
        body.extend(payload);
        body.push(0);
        let mut out = i32::try_from(body.len() + 4).unwrap().to_le_bytes().to_vec();
        out.extend(body);
        out
    }
}
