use std::io::Cursor;

use arrow_array::cast::AsArray;
use arrow_array::Array;
use arrow_ipc::reader::StreamReader;
use arrow_schema::{DataType, SchemaRef, TimeUnit};
use serde_json::Value;

use crate::decimal::format_scaled;
use crate::fail::{fail, Gate};
use crate::timefmt::format_unix_nanos;

pub struct Loaded {
    pub schema: SchemaRef,
    pub rows: Vec<Vec<Value>>,
}

pub fn load_ipc(bytes: &[u8]) -> Gate<Loaded> {
    if bytes.len() < 8 {
        return Err(fail("ipc is truncated: fewer than 8 bytes"));
    }
    let reader = StreamReader::try_new(Cursor::new(bytes), None)
        .map_err(|error| fail(format!("ipc schema is not a complete Arrow stream: {error}")))?;
    let schema = reader.schema();
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|error| fail(format!("ipc batch is truncated or invalid: {error}")))?;
        for row in 0..batch.num_rows() {
            let mut values = Vec::with_capacity(batch.num_columns());
            for column in batch.columns() {
                values.push(cell(column.as_ref(), row)?);
            }
            rows.push(values);
        }
    }
    Ok(Loaded { schema, rows })
}

pub fn cell(array: &dyn Array, index: usize) -> Gate<Value> {
    if array.is_null(index) {
        return Ok(Value::Null);
    }
    if let Some(value) = dictionary_cell(array, index)? {
        return Ok(value);
    }
    match array.data_type() {
        DataType::UInt64 | DataType::UInt32 | DataType::UInt16 | DataType::UInt8 => {
            Ok(Value::String(unsigned(array, index)?))
        }
        DataType::Int64 | DataType::Int32 | DataType::Int16 | DataType::Int8 => {
            Ok(Value::String(signed(array, index)?))
        }
        DataType::Float16 | DataType::Float32 | DataType::Float64 => {
            Err(fail("refusing float column; it is not an exact oracle"))
        }
        DataType::Decimal128(_, _) => Err(fail("refusing Decimal128; source precision does not fit")),
        DataType::Decimal256(_, scale) => {
            let text = array
                .as_primitive::<arrow_array::types::Decimal256Type>()
                .value(index)
                .to_string();
            Ok(Value::String(format_scaled(&text, *scale)?))
        }
        DataType::Timestamp(TimeUnit::Nanosecond, tz) => {
            if !tz_is_utc(tz) {
                return Err(fail(format!("timestamp timezone is not UTC: {tz:?}")));
            }
            let nanos = array
                .as_primitive::<arrow_array::types::TimestampNanosecondType>()
                .value(index);
            Ok(Value::String(format_unix_nanos(nanos)?))
        }
        DataType::Timestamp(unit, _) => Err(fail(format!(
            "timestamp unit {unit:?} is precision_loss against DateTime64(9); refusing the cast"
        ))),
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => Ok(Value::String(text_bytes(array, index)?)),
        DataType::Binary | DataType::LargeBinary | DataType::BinaryView => {
            let bytes = raw_bytes(array, index)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| fail("binary is not utf-8; refusing substitution"))?;
            Ok(Value::String(text.to_string()))
        }
        DataType::List(_) | DataType::LargeList(_) | DataType::ListView(_) | DataType::LargeListView(_) => {
            list_cells(array, index)
        }
        DataType::FixedSizeList(_, _) => list_cells(array, index),
        other => Err(fail(format!("unsupported arrow type {other}"))),
    }
}

pub fn raw_bytes(array: &dyn Array, index: usize) -> Gate<Vec<u8>> {
    if array.is_null(index) {
        return Err(fail("null has no bytes"));
    }
    if let Some(bytes) = dictionary_bytes(array, index)? {
        return Ok(bytes);
    }
    if let Some(values) = array.as_binary_opt::<i32>() {
        return Ok(values.value(index).to_vec());
    }
    if let Some(values) = array.as_binary_opt::<i64>() {
        return Ok(values.value(index).to_vec());
    }
    if let Some(values) = array.as_binary_view_opt() {
        return Ok(values.value(index).to_vec());
    }
    if let Some(values) = array.as_string_opt::<i32>() {
        return Ok(values.value(index).as_bytes().to_vec());
    }
    if let Some(values) = array.as_string_opt::<i64>() {
        return Ok(values.value(index).as_bytes().to_vec());
    }
    if let Some(values) = array.as_string_view_opt() {
        return Ok(values.value(index).as_bytes().to_vec());
    }
    let data_type = array.data_type();
    Err(fail(format!("column is not binary or utf8: {data_type}")))
}

fn text_bytes(array: &dyn Array, index: usize) -> Gate<String> {
    let bytes = raw_bytes(array, index)?;
    String::from_utf8(bytes).map_err(|_| fail("utf8 column is not utf-8"))
}

fn unsigned(array: &dyn Array, index: usize) -> Gate<String> {
    use arrow_array::types::{UInt16Type, UInt32Type, UInt64Type, UInt8Type};
    if let Some(values) = array.as_primitive_opt::<UInt64Type>() {
        return Ok(values.value(index).to_string());
    }
    if let Some(values) = array.as_primitive_opt::<UInt32Type>() {
        return Ok(u64::from(values.value(index)).to_string());
    }
    if let Some(values) = array.as_primitive_opt::<UInt16Type>() {
        return Ok(u64::from(values.value(index)).to_string());
    }
    if let Some(values) = array.as_primitive_opt::<UInt8Type>() {
        return Ok(u64::from(values.value(index)).to_string());
    }
    Err(fail("unsigned column did not downcast"))
}

fn signed(array: &dyn Array, index: usize) -> Gate<String> {
    use arrow_array::types::{Int16Type, Int32Type, Int64Type, Int8Type};
    if let Some(values) = array.as_primitive_opt::<Int64Type>() {
        return Ok(values.value(index).to_string());
    }
    if let Some(values) = array.as_primitive_opt::<Int32Type>() {
        return Ok(i64::from(values.value(index)).to_string());
    }
    if let Some(values) = array.as_primitive_opt::<Int16Type>() {
        return Ok(i64::from(values.value(index)).to_string());
    }
    if let Some(values) = array.as_primitive_opt::<Int8Type>() {
        return Ok(i64::from(values.value(index)).to_string());
    }
    Err(fail("signed column did not downcast"))
}

fn list_values(array: &dyn Array, index: usize) -> Option<arrow_array::ArrayRef> {
    if let Some(list) = array.as_list_opt::<i32>() {
        return Some(list.value(index));
    }
    if let Some(list) = array.as_list_opt::<i64>() {
        return Some(list.value(index));
    }
    if let Some(list) = array.as_list_view_opt::<i32>() {
        return Some(list.value(index));
    }
    if let Some(list) = array.as_list_view_opt::<i64>() {
        return Some(list.value(index));
    }
    if let Some(list) = array.as_fixed_size_list_opt() {
        return Some(list.value(index));
    }
    None
}

fn list_cells(array: &dyn Array, index: usize) -> Gate<Value> {
    let Some(values) = list_values(array, index) else {
        return Err(fail("list column did not downcast"));
    };
    let mut out = Vec::with_capacity(values.len());
    for child in 0..values.len() {
        out.push(cell(values.as_ref(), child)?);
    }
    Ok(Value::Array(out))
}

fn dictionary_cell(array: &dyn Array, index: usize) -> Gate<Option<Value>> {
    use arrow_array::types::{Int16Type, Int32Type, Int64Type, Int8Type, UInt16Type, UInt32Type, UInt8Type};
    let key = dictionary_key::<Int8Type>(array, index)
        .or(dictionary_key::<Int16Type>(array, index))
        .or(dictionary_key::<Int32Type>(array, index))
        .or(dictionary_key::<Int64Type>(array, index))
        .or(dictionary_key::<UInt8Type>(array, index))
        .or(dictionary_key::<UInt16Type>(array, index))
        .or(dictionary_key::<UInt32Type>(array, index));
    let Some(found) = key else {
        return Ok(None);
    };
    match found {
        DictKey::Absent => Ok(None),
        DictKey::Null => Ok(Some(Value::Null)),
        DictKey::Value(values, key) => Ok(Some(cell(values, key)?)),
    }
}

fn dictionary_bytes(array: &dyn Array, index: usize) -> Gate<Option<Vec<u8>>> {
    use arrow_array::types::{Int16Type, Int32Type, Int64Type, Int8Type, UInt16Type, UInt32Type, UInt8Type};
    let found = dictionary_key::<Int8Type>(array, index)
        .or(dictionary_key::<Int16Type>(array, index))
        .or(dictionary_key::<Int32Type>(array, index))
        .or(dictionary_key::<Int64Type>(array, index))
        .or(dictionary_key::<UInt8Type>(array, index))
        .or(dictionary_key::<UInt16Type>(array, index))
        .or(dictionary_key::<UInt32Type>(array, index));
    match found {
        None => Ok(None),
        Some(DictKey::Absent) => Err(fail("dictionary index is past the end")),
        Some(DictKey::Null) => Err(fail("null has no bytes")),
        Some(DictKey::Value(values, key)) => Ok(Some(raw_bytes(values, key)?)),
    }
}

enum DictKey<'a> {
    Absent,
    Null,
    Value(&'a dyn Array, usize),
}

fn dictionary_key<K>(array: &dyn Array, index: usize) -> Option<DictKey<'_>>
where
    K: arrow_array::types::ArrowDictionaryKeyType,
{
    let dict = array.as_dictionary_opt::<K>()?;
    match dict.keys_iter().nth(index) {
        Some(Some(key)) => Some(DictKey::Value(dict.values().as_ref(), key)),
        Some(None) => Some(DictKey::Null),
        None => Some(DictKey::Absent),
    }
}

pub fn tz_is_utc(tz: &Option<std::sync::Arc<str>>) -> bool {
    match tz {
        None => true,
        Some(text) => matches!(text.as_ref(), "UTC" | "Etc/UTC" | "+00:00" | "Z" | "UTC+0"),
    }
}

