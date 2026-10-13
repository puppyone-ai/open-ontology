//! Typed Arrow encoding for the W41 shipment batch.
//! Heterogeneous scalar `lane` is a dense union. Absent, null, and empty stay distinct
//! through an explicit presence byte. The Arrow stream has no whole-row JSON column.
//! `canonical_raw_documents` is a sidecar of the same read. It is not a PostgreSQL jsonb seal.

use std::sync::Arc;

use arrow_array::Array;
use arrow_array::ArrayRef;
use arrow_array::FixedSizeBinaryArray;
use arrow_array::Float64Array;
use arrow_array::Int32Array;
use arrow_array::Int64Array;
use arrow_array::RecordBatch;
use arrow_array::StringArray;
use arrow_array::StructArray;
use arrow_array::TimestampMillisecondArray;
use arrow_array::UInt8Array;
use arrow_array::UnionArray;
use arrow_array::builder::BinaryBuilder;
use arrow_array::builder::ListBuilder;
use arrow_array::builder::StringBuilder;
use arrow_buffer::Buffer;
use arrow_buffer::NullBuffer;
use arrow_buffer::ScalarBuffer;
use arrow_ipc::writer::FileWriter;
use arrow_schema::DataType;
use arrow_schema::Field;
use arrow_schema::Fields;
use arrow_schema::Schema;
use arrow_schema::UnionFields;
use mongodb::bson::Binary;
use mongodb::bson::Bson;
use mongodb::bson::Document;
use mongodb::bson::spec::BinarySubtype;
use serde_json::Value;
use serde_json::json;

pub const PRICE_SCALE: i32 = 7;
pub const PRICE_PRECISION: usize = 34;
pub const PRESENCE_ABSENT: u8 = 0;
pub const PRESENCE_NULL: u8 = 1;
pub const PRESENCE_EMPTY: u8 = 2;
pub const PRESENCE_VALUE: u8 = 3;

pub struct ShipmentRow {
    pub oid: [u8; 12],
    pub code: String,
    pub flag: i32,
    pub units: i64,
    pub price_unscaled: i128,
    pub created_ms: i64,
    pub uid: [u8; 16],
    pub blob: Vec<u8>,
    pub blob_presence: u8,
    pub weight: f64,
    pub warehouse: String,
    pub wh_code: String,
    pub lane_text: Option<String>,
    pub lane_num: Option<i64>,
    pub memo: Option<String>,
    pub memo_presence: u8,
    pub marks: Option<Vec<String>>,
    pub marks_presence: u8,
    pub pack_kind: Option<String>,
    pub pack_n: Option<i32>,
    pub pack_presence: u8,
}

pub fn parse_shipment(doc: &Document) -> Result<ShipmentRow, String> {
    let oid = match doc.get("_id") {
        Some(Bson::ObjectId(oid)) => oid.bytes(),
        other => return Err(format!("_id {}", type_name(other))),
    };
    let code = require_string(doc, "code")?;
    let flag = require_i32(doc, "flag")?;
    let units = require_i64(doc, "units")?;
    let price_unscaled = match doc.get("price") {
        Some(Bson::Decimal128(value)) => unscaled_from_display(&value.to_string())?,
        other => return Err(format!("price {}", type_name(other))),
    };
    let created_ms = match doc.get("created") {
        Some(Bson::DateTime(value)) => value.timestamp_millis(),
        other => return Err(format!("created {}", type_name(other))),
    };
    let uid = require_binary(doc, "uid", 4, Some(16))?;
    let mut uid_bytes = [0u8; 16];
    uid_bytes.copy_from_slice(&uid);
    let (blob, blob_presence) = match doc.get("blob") {
        None => (Vec::new(), PRESENCE_ABSENT),
        Some(Bson::Null) => (Vec::new(), PRESENCE_NULL),
        Some(Bson::Binary(value)) => {
            if subtype_byte(value.subtype) != 0 {
                return Err(format!("blob subtype {}", subtype_byte(value.subtype)));
            }
            let presence = if value.bytes.is_empty() {
                PRESENCE_EMPTY
            } else {
                PRESENCE_VALUE
            };
            (value.bytes.clone(), presence)
        }
        other => return Err(format!("blob {}", type_name(other))),
    };
    let weight = match doc.get("weight") {
        Some(Bson::Double(value)) => *value,
        other => return Err(format!("weight {}", type_name(other))),
    };
    let warehouse = require_string(doc, "warehouse")?;
    let wh_code = require_string(doc, "whCode")?;
    let (lane_text, lane_num) = match doc.get("lane") {
        Some(Bson::String(value)) => (Some(value.clone()), None),
        Some(Bson::Int64(value)) => (None, Some(*value)),
        other => return Err(format!("lane {}", type_name(other))),
    };
    let (memo, memo_presence) = match doc.get("memo") {
        None => (None, PRESENCE_ABSENT),
        Some(Bson::Null) => (None, PRESENCE_NULL),
        Some(Bson::String(value)) if value.is_empty() => (Some(String::new()), PRESENCE_EMPTY),
        Some(Bson::String(value)) => (Some(value.clone()), PRESENCE_VALUE),
        other => return Err(format!("memo {}", type_name(other))),
    };
    let (marks, marks_presence) = parse_marks(doc)?;
    let (pack_kind, pack_n, pack_presence) = parse_pack(doc)?;
    Ok(ShipmentRow {
        oid,
        code,
        flag,
        units,
        price_unscaled,
        created_ms,
        uid: uid_bytes,
        blob,
        blob_presence,
        weight,
        warehouse,
        wh_code,
        lane_text,
        lane_num,
        memo,
        memo_presence,
        marks,
        marks_presence,
        pack_kind,
        pack_n,
        pack_presence,
    })
}

pub fn reject_decimal(doc: &Document) -> Result<Value, String> {
    let code = require_string(doc, "code")?;
    let Some(price) = doc.get("price") else {
        return Err(format!("{code} missing price"));
    };
    let Bson::Decimal128(value) = price else {
        return Err(format!("{code} price {}", type_name(Some(price))));
    };
    let text = value.to_string();
    match unscaled_from_display(&text) {
        Err(reason) if reason.starts_with("DECIMAL128_NON_FINITE") => Ok(json!({
            "code": code,
            "reason": "DECIMAL128_NON_FINITE",
            "detail": text,
        })),
        Err(reason) if reason.starts_with("DECIMAL128_PRECISION") => Ok(json!({
            "code": code,
            "reason": "DECIMAL128_PRECISION",
            "detail": text,
        })),
        Ok(_) => Err(format!("{code} finite decimal was not rejected: {text}")),
        Err(reason) => Err(format!("{code} {reason}")),
    }
}

pub fn reject_marks(doc: &Document) -> Result<Value, String> {
    let code = require_string(doc, "code")?;
    let Some(marks) = doc.get("marks") else {
        return Err(format!("{code} missing marks"));
    };
    let Bson::Array(items) = marks else {
        return Err(format!("{code} marks {}", type_name(Some(marks))));
    };
    let types: Vec<&str> = items.iter().map(bson_type_name).collect();
    let mut distinct = types.clone();
    distinct.sort_unstable();
    distinct.dedup();
    if distinct.len() > 1 {
        return Ok(json!({
            "code": code,
            "reason": "HETEROGENEOUS_ARRAY_ELEMENT",
            "types": types,
        }));
    }
    Err(format!("{code} marks were homogeneous: {types:?}"))
}

pub fn field_census(docs: &[Document]) -> Value {
    let mut names = Vec::new();
    for doc in docs {
        for key in doc.keys() {
            if !names.iter().any(|existing| existing == key) {
                names.push(key.clone());
            }
        }
    }
    let mut fields = Vec::new();
    for name in names {
        let mut types = Vec::new();
        let mut present = 0u64;
        for doc in docs {
            if let Some(value) = doc.get(&name) {
                present += 1;
                let label = bson_type_name(value).to_string();
                if !types.contains(&label) {
                    types.push(label);
                }
            }
        }
        fields.push(json!({
            "field": name,
            "present": present,
            "documents": docs.len(),
            "bson_types": types,
        }));
    }
    json!({
        "sample_not_enforced_schema": true,
        "schema_source": "sample_field_census",
        "documents": docs.len(),
        "fields": fields,
    })
}

pub fn encode_batch(rows: &[ShipmentRow]) -> Result<RecordBatch, String> {
    let mut oid_bytes = Vec::with_capacity(rows.len() * 12);
    let mut uid_bytes = Vec::with_capacity(rows.len() * 16);
    let mut codes = Vec::with_capacity(rows.len());
    let mut flags = Vec::with_capacity(rows.len());
    let mut units = Vec::with_capacity(rows.len());
    let mut prices = Vec::with_capacity(rows.len());
    let mut created = Vec::with_capacity(rows.len());
    let mut blob_presence = Vec::with_capacity(rows.len());
    let mut weights = Vec::with_capacity(rows.len());
    let mut warehouses = Vec::with_capacity(rows.len());
    let mut wh_codes = Vec::with_capacity(rows.len());
    let mut lane_type_ids = Vec::with_capacity(rows.len());
    let mut lane_offsets = Vec::with_capacity(rows.len());
    let mut lane_text = Vec::new();
    let mut lane_num = Vec::new();
    let mut memos = Vec::with_capacity(rows.len());
    let mut memo_presence = Vec::with_capacity(rows.len());
    let mut marks_presence = Vec::with_capacity(rows.len());
    let mut pack_kind = Vec::with_capacity(rows.len());
    let mut pack_n = Vec::with_capacity(rows.len());
    let mut pack_valid = Vec::with_capacity(rows.len());
    let mut pack_presence = Vec::with_capacity(rows.len());
    let mut marks_builder = ListBuilder::new(StringBuilder::new());
    let mut blob_builder = BinaryBuilder::new();

    for row in rows {
        oid_bytes.extend_from_slice(&row.oid);
        uid_bytes.extend_from_slice(&row.uid);
        codes.push(row.code.as_str());
        flags.push(row.flag);
        units.push(row.units);
        prices.push(row.price_unscaled);
        created.push(row.created_ms);
        blob_builder.append_value(&row.blob);
        blob_presence.push(row.blob_presence);
        weights.push(row.weight);
        warehouses.push(row.warehouse.as_str());
        wh_codes.push(row.wh_code.as_str());
        match (&row.lane_text, row.lane_num) {
            (Some(text), None) => {
                lane_type_ids.push(0i8);
                lane_offsets.push(i32::try_from(lane_text.len()).map_err(|err| err.to_string())?);
                lane_text.push(text.as_str());
            }
            (None, Some(number)) => {
                lane_type_ids.push(1i8);
                lane_offsets.push(i32::try_from(lane_num.len()).map_err(|err| err.to_string())?);
                lane_num.push(number);
            }
            _ => return Err(format!("lane both or neither for {}", row.code)),
        }
        memos.push(match row.memo_presence {
            PRESENCE_VALUE | PRESENCE_EMPTY => row.memo.clone(),
            _ => None,
        });
        memo_presence.push(row.memo_presence);
        match &row.marks {
            Some(items) => {
                for item in items {
                    marks_builder.values().append_value(item);
                }
                marks_builder.append(true);
            }
            None => marks_builder.append(false),
        }
        marks_presence.push(row.marks_presence);
        pack_kind.push(row.pack_kind.clone());
        pack_n.push(row.pack_n);
        pack_valid.push(row.pack_presence >= PRESENCE_EMPTY);
        pack_presence.push(row.pack_presence);
    }

    let oid = FixedSizeBinaryArray::try_new(12, Buffer::from(oid_bytes), None).map_err(|err| err.to_string())?;
    let code = StringArray::from(codes);
    let flag = Int32Array::from(flags);
    let units_array = Int64Array::from(units);
    let price = arrow_array::Decimal128Array::from(prices)
        .with_precision_and_scale(34, 7)
        .map_err(|err| err.to_string())?;
    let created_array = TimestampMillisecondArray::from(created).with_timezone("UTC");
    let uid = FixedSizeBinaryArray::try_new(16, Buffer::from(uid_bytes), None).map_err(|err| err.to_string())?;
    let blob = blob_builder.finish();
    let blob_presence_array = UInt8Array::from(blob_presence);
    let weight = Float64Array::from(weights);
    let warehouse = StringArray::from(warehouses);
    let wh_code = StringArray::from(wh_codes);
    let lane = dense_lane(&lane_type_ids, &lane_offsets, &lane_text, &lane_num)?;
    let memo = StringArray::from(memos);
    let memo_presence_array = UInt8Array::from(memo_presence);
    let marks = marks_builder.finish();
    let marks_presence_array = UInt8Array::from(marks_presence);
    let pack = pack_array(&pack_kind, &pack_n, &pack_valid)?;
    let pack_presence_array = UInt8Array::from(pack_presence);

    let columns: Vec<ArrayRef> = vec![
        Arc::new(oid),
        Arc::new(code),
        Arc::new(flag),
        Arc::new(units_array),
        Arc::new(price),
        Arc::new(created_array),
        Arc::new(uid),
        Arc::new(blob),
        Arc::new(blob_presence_array),
        Arc::new(weight),
        Arc::new(warehouse),
        Arc::new(wh_code),
        Arc::new(lane),
        Arc::new(memo),
        Arc::new(memo_presence_array),
        Arc::new(marks),
        Arc::new(marks_presence_array),
        Arc::new(pack),
        Arc::new(pack_presence_array),
    ];
    let fields: Vec<Field> = columns
        .iter()
        .zip(COLUMN_NAMES)
        .map(|(column, name)| {
            let nullable = matches!(name, "memo" | "marks" | "pack");
            Field::new(name, column.data_type().clone(), nullable)
        })
        .collect();
    let mut metadata = std::collections::HashMap::new();
    metadata.insert("encoder".to_string(), "oo-dl01w1-41-typed-arrow".to_string());
    metadata.insert("json_document_column".to_string(), "absent".to_string());
    let schema = Arc::new(Schema::new_with_metadata(fields, metadata));
    RecordBatch::try_new(schema, columns).map_err(|err| err.to_string())
}

pub fn column_names() -> &'static [&'static str] {
    &COLUMN_NAMES
}

/// Canonical extended JSON for the rows already read. `pg_landed` stays false:
/// `copy_encoder` rejects oid 3802, and this sidecar is not a table.
pub fn canonical_raw_documents(docs: &[Document]) -> Result<Value, String> {
    let mut rows = Vec::with_capacity(docs.len());
    for doc in docs {
        let ext = Bson::Document(doc.clone()).into_canonical_extjson();
        rows.push(serde_json::to_value(&ext).map_err(|err| err.to_string())?);
    }
    Ok(json!({
        "protocol": "oo.mongodb-preview.raw-document/v1",
        "rows": rows.len(),
        "pg_type": "jsonb",
        "pg_oid": 3802,
        "pg_landed": false,
        "blocker": "copy_encoder oid 3802 type_not_encoded",
        "owners": ["root", "DL01-W1-05", "DL01-W1-06", "DL01-W1-21"],
        "documents": rows,
    }))
}

const COLUMN_NAMES: [&str; 19] = [
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

pub fn ipc_bytes(batch: &RecordBatch) -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    {
        let mut writer = FileWriter::try_new(&mut buffer, batch.schema().as_ref()).map_err(|err| err.to_string())?;
        writer.write(batch).map_err(|err| err.to_string())?;
        writer.finish().map_err(|err| err.to_string())?;
    }
    Ok(buffer)
}

pub fn unscaled_from_display(text: &str) -> Result<i128, String> {
    if matches!(text, "NaN" | "Infinity" | "-Infinity" | "Inf" | "-Inf" | "sNaN") {
        return Err(format!("DECIMAL128_NON_FINITE:{text}"));
    }
    let (mantissa, exponent) = split_exponent(text)?;
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.trim_start_matches(['+', '-']);
    let (whole, frac) = match mantissa.split_once('.') {
        Some((whole, frac)) => (whole, frac),
        None => (mantissa, ""),
    };
    if whole.is_empty()
        || !whole.chars().all(|ch| ch.is_ascii_digit())
        || !frac.chars().all(|ch| ch.is_ascii_digit())
    {
        return Err(format!("DECIMAL128_PRECISION:{text}"));
    }
    let digits_raw = format!("{whole}{frac}");
    let frac_len = i32::try_from(frac.len()).map_err(|_| format!("DECIMAL128_PRECISION:{text}"))?;
    let scale = frac_len - exponent;
    let trimmed = digits_raw.trim_start_matches('0');
    if trimmed.is_empty() {
        return Ok(0);
    }
    if trimmed.len() > PRICE_PRECISION {
        return Err(format!("DECIMAL128_PRECISION:{text}"));
    }
    if scale > PRICE_SCALE {
        let extra = usize::try_from(scale - PRICE_SCALE).map_err(|_| format!("DECIMAL128_PRECISION:{text}"))?;
        if extra > trimmed.len() {
            return Err(format!("DECIMAL128_PRECISION:{text}"));
        }
        let (keep, drop) = trimmed.split_at(trimmed.len() - extra);
        if drop.chars().any(|ch| ch != '0') {
            return Err(format!("DECIMAL128_PRECISION:{text}"));
        }
        let keep = if keep.is_empty() { "0" } else { keep };
        return parse_coeff(keep, negative, text);
    }
    let pad = usize::try_from(PRICE_SCALE - scale).map_err(|_| format!("DECIMAL128_PRECISION:{text}"))?;
    if trimmed.len() + pad > PRICE_PRECISION {
        return Err(format!("DECIMAL128_PRECISION:{text}"));
    }
    let mut coeff = trimmed.to_string();
    coeff.extend(std::iter::repeat_n('0', pad));
    parse_coeff(&coeff, negative, text)
}

fn parse_marks(doc: &Document) -> Result<(Option<Vec<String>>, u8), String> {
    match doc.get("marks") {
        None => Ok((None, PRESENCE_ABSENT)),
        Some(Bson::Null) => Ok((None, PRESENCE_NULL)),
        Some(Bson::Array(items)) if items.is_empty() => Ok((Some(Vec::new()), PRESENCE_EMPTY)),
        Some(Bson::Array(items)) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Bson::String(value) => values.push(value.clone()),
                    _ => {
                        return Err(format!(
                            "HETEROGENEOUS_ARRAY_ELEMENT marks {}",
                            bson_type_name(item)
                        ));
                    }
                }
            }
            Ok((Some(values), PRESENCE_VALUE))
        }
        other => Err(format!("marks {}", type_name(other))),
    }
}

fn parse_pack(doc: &Document) -> Result<(Option<String>, Option<i32>, u8), String> {
    match doc.get("pack") {
        None => Ok((None, None, PRESENCE_ABSENT)),
        Some(Bson::Null) => Ok((None, None, PRESENCE_NULL)),
        Some(Bson::Document(nested)) if nested.is_empty() => Ok((None, None, PRESENCE_EMPTY)),
        Some(Bson::Document(nested)) => {
            let mut keys: Vec<&String> = nested.keys().collect();
            keys.sort();
            if keys.iter().copied().map(String::as_str).collect::<Vec<_>>() != ["kind", "n"] {
                return Err(format!("pack keys {keys:?}"));
            }
            let kind = match nested.get("kind") {
                Some(Bson::String(value)) => value.clone(),
                other => return Err(format!("pack.kind {}", type_name(other))),
            };
            let n = match nested.get("n") {
                Some(Bson::Int32(value)) => *value,
                other => return Err(format!("pack.n {}", type_name(other))),
            };
            Ok((Some(kind), Some(n), PRESENCE_VALUE))
        }
        other => Err(format!("pack {}", type_name(other))),
    }
}

fn dense_lane(
    type_ids: &[i8],
    offsets: &[i32],
    text: &[&str],
    numbers: &[i64],
) -> Result<UnionArray, String> {
    let fields = UnionFields::try_new(
        vec![0i8, 1i8],
        vec![
            Field::new("text", DataType::Utf8, false),
            Field::new("num", DataType::Int64, false),
        ],
    )
    .map_err(|err| err.to_string())?;
    let children: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from(text.to_vec())),
        Arc::new(Int64Array::from(numbers.to_vec())),
    ];
    UnionArray::try_new(
        fields,
        ScalarBuffer::from(type_ids.to_vec()),
        Some(ScalarBuffer::from(offsets.to_vec())),
        children,
    )
    .map_err(|err| err.to_string())
}

fn pack_array(kind: &[Option<String>], n: &[Option<i32>], valid: &[bool]) -> Result<StructArray, String> {
    let kind_array = Arc::new(StringArray::from(kind.to_vec())) as ArrayRef;
    let n_array = Arc::new(Int32Array::from(n.to_vec())) as ArrayRef;
    let fields = Fields::from(vec![
        Field::new("kind", kind_array.data_type().clone(), true),
        Field::new("n", n_array.data_type().clone(), true),
    ]);
    let nulls = if valid.iter().any(|item| !item) {
        Some(NullBuffer::from_iter(valid.iter().copied()))
    } else {
        None
    };
    StructArray::try_new(fields, vec![kind_array, n_array], nulls).map_err(|err| err.to_string())
}

fn require_string(doc: &Document, key: &str) -> Result<String, String> {
    match doc.get(key) {
        Some(Bson::String(value)) => Ok(value.clone()),
        other => Err(format!("{key} {}", type_name(other))),
    }
}

fn require_i32(doc: &Document, key: &str) -> Result<i32, String> {
    match doc.get(key) {
        Some(Bson::Int32(value)) => Ok(*value),
        other => Err(format!("{key} {}", type_name(other))),
    }
}

fn require_i64(doc: &Document, key: &str) -> Result<i64, String> {
    match doc.get(key) {
        Some(Bson::Int64(value)) => Ok(*value),
        other => Err(format!("{key} {}", type_name(other))),
    }
}

fn require_binary(doc: &Document, key: &str, subtype: u8, len: Option<usize>) -> Result<Vec<u8>, String> {
    match doc.get(key) {
        Some(Bson::Binary(value)) => binary_bytes(key, value, subtype, len),
        other => Err(format!("{key} {}", type_name(other))),
    }
}

fn binary_bytes(key: &str, value: &Binary, subtype: u8, len: Option<usize>) -> Result<Vec<u8>, String> {
    let actual = subtype_byte(value.subtype);
    if actual != subtype {
        return Err(format!("{key} subtype {actual}"));
    }
    if let Some(expected) = len {
        if value.bytes.len() != expected {
            return Err(format!("{key} length {}", value.bytes.len()));
        }
    }
    Ok(value.bytes.clone())
}

fn subtype_byte(subtype: BinarySubtype) -> u8 {
    u8::from(subtype)
}

fn type_name(value: Option<&Bson>) -> String {
    match value {
        None => "absent".to_string(),
        Some(bson) => bson_type_name(bson).to_string(),
    }
}

fn bson_type_name(value: &Bson) -> &'static str {
    match value {
        Bson::Double(_) => "double",
        Bson::String(_) => "string",
        Bson::Array(_) => "array",
        Bson::Document(_) => "document",
        Bson::Boolean(_) => "bool",
        Bson::Null => "null",
        Bson::RegularExpression(_) => "regex",
        Bson::JavaScriptCode(_) => "javascript",
        Bson::JavaScriptCodeWithScope(_) => "javascript_with_scope",
        Bson::Int32(_) => "int32",
        Bson::Int64(_) => "int64",
        Bson::Timestamp(_) => "timestamp",
        Bson::Binary(_) => "binary",
        Bson::ObjectId(_) => "objectId",
        Bson::DateTime(_) => "date",
        Bson::Symbol(_) => "symbol",
        Bson::Decimal128(_) => "decimal128",
        Bson::Undefined => "undefined",
        Bson::MaxKey => "maxKey",
        Bson::MinKey => "minKey",
        Bson::DbPointer(_) => "dbPointer",
    }
}

fn split_exponent(text: &str) -> Result<(&str, i32), String> {
    let Some(index) = text.find(['e', 'E']) else {
        return Ok((text, 0));
    };
    let (mantissa, exponent) = text.split_at(index);
    let exponent = &exponent[1..];
    let parsed = exponent.parse::<i32>().map_err(|_| format!("DECIMAL128_PRECISION:{text}"))?;
    Ok((mantissa, parsed))
}

fn parse_coeff(digits: &str, negative: bool, text: &str) -> Result<i128, String> {
    let value = digits.parse::<i128>().map_err(|_| format!("DECIMAL128_PRECISION:{text}"))?;
    if negative { Ok(-value) } else { Ok(value) }
}

#[cfg(test)]
mod tests {
    use super::unscaled_from_display;

    #[test]
    fn plain_price_keeps_scale_7() {
        let value = unscaled_from_display("314159265358979.0000041").unwrap();
        assert_eq!(value, 3_141_592_653_589_790_000_041);
    }

    #[test]
    fn scientific_price_matches_plain() {
        let value = unscaled_from_display("3.141592653589790000041E+14").unwrap();
        assert_eq!(value, 3_141_592_653_589_790_000_041);
    }

    #[test]
    fn non_finite_is_rejected() {
        for text in ["NaN", "Infinity", "-Infinity"] {
            let err = unscaled_from_display(text).unwrap_err();
            assert!(err.contains("NON_FINITE"), "{text} -> {err}");
        }
    }

    #[test]
    fn extra_nonzero_fraction_is_rejected() {
        let err = unscaled_from_display("1.00000001").unwrap_err();
        assert!(err.contains("PRECISION"), "{err}");
    }
}
