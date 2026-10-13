#![forbid(unsafe_code)]

mod civil;
mod decimal;

use std::fs;
use std::path::Path;

use arrow_array::array::{Array, PrimitiveArray};
use arrow_array::types::TimestampMicrosecondType;
use arrow_array::{
    BinaryArray, Date32Array, Decimal128Array, Decimal256Array, Int16Array, Int32Array, Int64Array,
    Int8Array, RecordBatch, StringArray, UInt16Array, UInt32Array, UInt64Array, UInt8Array,
};
use arrow_ipc::reader::StreamReader;
use arrow_schema::{DataType, Field, TimeUnit};
use serde_json::{json, Value as Json};

pub const ROWS_PROTOCOL: &str = "oo.mariadb-driver-preflight.rows/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeFail {
    pub fact: String,
}

impl std::fmt::Display for DecodeFail {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.fact)
    }
}

pub fn decode_files(paths: &[impl AsRef<Path>]) -> Result<Json, DecodeFail> {
    let mut tables = Vec::new();
    for path in paths {
        tables.push(decode_path(path.as_ref())?);
    }
    tables.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    Ok(json!({
        "protocol": ROWS_PROTOCOL,
        "tables": tables,
    }))
}

pub fn decode_path(path: &Path) -> Result<Json, DecodeFail> {
    let bytes = fs::read(path).map_err(|err| DecodeFail { fact: err.to_string() })?;
    decode_bytes(&bytes)
}

pub fn decode_bytes(bytes: &[u8]) -> Result<Json, DecodeFail> {
    let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None)
        .map_err(|err| DecodeFail { fact: err.to_string() })?;
    let schema = reader.schema();
    let mut batches = Vec::new();
    for batch in reader {
        batches.push(batch.map_err(|err| DecodeFail { fact: err.to_string() })?);
    }
    if batches.is_empty() {
        return Err(DecodeFail { fact: "ipc stream had no batch".to_string() });
    }
    table_json(schema.as_ref(), &batches)
}

fn table_json(schema: &arrow_schema::Schema, batches: &[RecordBatch]) -> Result<Json, DecodeFail> {
    let mut columns = Vec::new();
    let mut arrows = Vec::new();
    let mut physical = Vec::new();
    for field in schema.fields() {
        columns.push(Json::String(field.name().to_string()));
        arrows.push(Json::String(arrow_tag(field)));
        physical.push(Json::String(physical_name(field.data_type())?));
    }
    let mut rows = Vec::new();
    for batch in batches {
        if batch.num_columns() != schema.fields().len() {
            return Err(DecodeFail { fact: "batch width".to_string() });
        }
        for row_index in 0..batch.num_rows() {
            let mut row = Vec::new();
            for (column_index, field) in schema.fields().iter().enumerate() {
                row.push(cell(batch.column(column_index).as_ref(), row_index, field)?);
            }
            rows.push(Json::Array(row));
        }
    }
    let name = schema
        .metadata()
        .get("oo.mariadb.table")
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    Ok(json!({
        "name": name,
        "columns": columns,
        "arrow": arrows,
        "physical": physical,
        "rows": rows,
        "schema_metadata": metadata_json(schema.metadata()),
    }))
}

fn metadata_json(metadata: &arrow_schema::Metadata) -> Json {
    let mut object = serde_json::Map::new();
    for (key, value) in metadata.iter() {
        object.insert(key.clone(), Json::String(value.clone()));
    }
    Json::Object(object)
}

fn arrow_tag(field: &Field) -> String {
    field
        .metadata()
        .get("oo.mariadb.arrow")
        .cloned()
        .unwrap_or_else(|| physical_name(field.data_type()).unwrap_or_else(|_| "unknown".to_string()))
}

fn physical_name(data_type: &DataType) -> Result<String, DecodeFail> {
    Ok(match data_type {
        DataType::Int8 => "int8",
        DataType::Int16 => "int16",
        DataType::Int32 => "int32",
        DataType::Int64 => "int64",
        DataType::UInt8 => "uint8",
        DataType::UInt16 => "uint16",
        DataType::UInt32 => "uint32",
        DataType::UInt64 => "uint64",
        DataType::Utf8 => "utf8",
        DataType::Binary => "binary",
        DataType::Date32 => "date32",
        DataType::Timestamp(TimeUnit::Microsecond, None) => "timestamp_us_naive",
        DataType::Timestamp(TimeUnit::Microsecond, Some(zone)) if zone.as_ref() == "+00:00" => "timestamp_us_+00:00",
        DataType::Decimal128(precision, scale) => return Ok(format!("decimal128({precision},{scale})")),
        DataType::Decimal256(precision, scale) => return Ok(format!("decimal256({precision},{scale})")),
        other => return Err(DecodeFail { fact: format!("unsupported arrow {other:?}") }),
    }
    .to_string())
}

fn cell(array: &dyn Array, index: usize, field: &Field) -> Result<Json, DecodeFail> {
    if array.is_null(index) {
        return Ok(Json::Null);
    }
    let fail = |fact: &str| DecodeFail { fact: fact.to_string() };
    match array.data_type() {
        DataType::UInt64 => Ok(Json::String(downcast::<UInt64Array>(array, "uint64")?.value(index).to_string())),
        DataType::UInt32 => Ok(Json::String(downcast::<UInt32Array>(array, "uint32")?.value(index).to_string())),
        DataType::UInt16 => Ok(Json::String(downcast::<UInt16Array>(array, "uint16")?.value(index).to_string())),
        DataType::UInt8 => Ok(Json::String(downcast::<UInt8Array>(array, "uint8")?.value(index).to_string())),
        DataType::Int64 => {
            let number = downcast::<Int64Array>(array, "int64")?.value(index);
            if field.metadata().get("oo.mariadb.arrow").map(String::as_str) == Some("int64_interval_us") {
                Ok(Json::String(civil::format_interval(number)))
            } else {
                Ok(Json::String(number.to_string()))
            }
        }
        DataType::Int32 => Ok(Json::String(downcast::<Int32Array>(array, "int32")?.value(index).to_string())),
        DataType::Int16 => Ok(Json::String(downcast::<Int16Array>(array, "int16")?.value(index).to_string())),
        DataType::Int8 => Ok(Json::String(downcast::<Int8Array>(array, "int8")?.value(index).to_string())),
        DataType::Utf8 => Ok(Json::String(downcast::<StringArray>(array, "utf8")?.value(index).to_string())),
        DataType::Binary => Ok(Json::String(hex_upper(downcast::<BinaryArray>(array, "binary")?.value(index)))),
        DataType::Date32 => Ok(Json::String(civil::format_date(downcast::<Date32Array>(array, "date32")?.value(index)).map_err(|err| fail(&err))?)) ,
        DataType::Timestamp(TimeUnit::Microsecond, _) => {
            let number = downcast::<PrimitiveArray<TimestampMicrosecondType>>(array, "timestamp")?.value(index);
            Ok(Json::String(civil::format_timestamp(number).map_err(|err| fail(&err))?))
        }
        DataType::Decimal128(_, scale) => {
            let number = downcast::<Decimal128Array>(array, "decimal128")?.value(index);
            let scale = u8::try_from(*scale).map_err(|_| fail("scale"))?;
            Ok(Json::String(decimal::format_i128(number, scale).map_err(|err| fail(&err))?))
        }
        DataType::Decimal256(_, scale) => {
            let number = downcast::<Decimal256Array>(array, "decimal256")?.value(index);
            let scale = u8::try_from(*scale).map_err(|_| fail("scale"))?;
            Ok(Json::String(decimal::format_bytes(&number.to_le_bytes(), scale).map_err(|err| fail(&err))?))
        }
        other => Err(fail(&format!("{other:?}"))),
    }
}

fn downcast<'a, T: 'static>(array: &'a dyn Array, name: &str) -> Result<&'a T, DecodeFail> {
    array
        .as_any()
        .downcast_ref::<T>()
        .ok_or_else(|| DecodeFail { fact: format!("downcast {name}") })
}

fn hex_upper(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

pub fn publish(path: &Path, value: &Json) -> Result<(), DecodeFail> {
    let tmp = path.with_extension("json.tmp");
    let mut text = serde_json::to_vec_pretty(value).map_err(|err| DecodeFail { fact: err.to_string() })?;
    text.push(b'\n');
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| DecodeFail { fact: err.to_string() })?;
    }
    fs::write(&tmp, &text).map_err(|err| DecodeFail { fact: err.to_string() })?;
    fs::rename(&tmp, path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        DecodeFail { fact: err.to_string() }
    })
}

pub fn epoch_day_zero() -> i32 {
    civil::days_from_civil(1970, 1, 1).unwrap_or(i32::MIN)
}
