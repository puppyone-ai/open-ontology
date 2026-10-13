//! Independent Arrow IPC reader. It does not link the OpenSearch encoder.

#![allow(clippy::uninlined_format_args, clippy::redundant_closure_for_method_calls)]

use std::env;
use std::fs::File;
use std::io::BufReader;
use std::process::ExitCode;

use arrow_array::cast::as_list_array;
use arrow_array::types::{TimestampMicrosecondType, TimestampMillisecondType};
use arrow_array::{
    Array, BinaryArray, BooleanArray, Float64Array, Int64Array, PrimitiveArray, RecordBatch, StringArray,
    StructArray,
};
use arrow_ipc::reader::StreamReader;
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use serde_json::{json, Map, Value};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::from(0),
        Err(error) => {
            eprintln!("os-decode: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut ipc = None;
    let mut out = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next().ok_or_else(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--ipc" => ipc = Some(value),
            "--out" => out = Some(value),
            other => return Err(format!("unknown arg {other}")),
        }
    }
    let ipc = ipc.ok_or("missing --ipc")?;
    let out = out.ok_or("missing --out")?;
    let file = File::open(&ipc).map_err(|error| format!("open:{error}"))?;
    let reader = StreamReader::try_new(BufReader::new(file), None).map_err(|error| format!("arrow:{error}"))?;
    let schema = reader.schema();
    let mut batches = Vec::new();
    for batch in reader {
        batches.push(batch.map_err(|error| format!("batch:{error}"))?);
    }
    let rows = decode_batches(&schema, &batches)?;
    let document = json!({
        "batch_count": batches.len(),
        "schema_metadata": metadata_json(schema.metadata()),
        "fields": schema.fields().iter().map(|field| field_json(field.as_ref())).collect::<Vec<_>>(),
        "rows": rows,
    });
    if let Some(parent) = std::path::Path::new(&out).parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("mkdir:{error}"))?;
    }
    std::fs::write(&out, serde_json::to_string_pretty(&document).map_err(|error| error.to_string())? + "\n")
        .map_err(|error| format!("write:{error}"))?;
    Ok(())
}

fn metadata_json(metadata: &arrow_schema::Metadata) -> Map<String, Value> {
    let mut map = Map::new();
    for (key, value) in metadata.iter() {
        map.insert(key.clone(), Value::String(value.clone()));
    }
    map
}

fn field_json(field: &Field) -> Value {
    json!({
        "name": field.name(),
        "data_type": format!("{:?}", field.data_type()),
        "nullable": field.is_nullable(),
        "metadata": metadata_json(field.metadata()),
    })
}

fn decode_batches(schema: &Schema, batches: &[RecordBatch]) -> Result<Vec<Value>, String> {
    let mut rows = Vec::new();
    for batch in batches {
        if batch.schema().fields() != schema.fields() {
            return Err("batch schema changed".into());
        }
        for row in 0..batch.num_rows() {
            rows.push(decode_row(schema, batch, row)?);
        }
    }
    Ok(rows)
}

fn decode_row(schema: &Schema, batch: &RecordBatch, row: usize) -> Result<Value, String> {
    let id = string_at(batch.column(0), row)?;
    let index = string_at(batch.column(1), row)?;
    let raw = binary_at(batch.column(2), row)?;
    let mut columns = Map::new();
    let mut cursor = 3;
    while cursor + 1 < schema.fields().len() {
        let field = schema.field(cursor);
        let present = bool_at(batch.column(cursor + 1), row)?;
        let value = emit(field, batch.column(cursor).as_ref(), present, row)?;
        columns.insert(field.name().clone(), value);
        cursor += 2;
    }
    if cursor != schema.fields().len() {
        return Err("column pairs are uneven".into());
    }
    Ok(json!({
        "source_doc_id": id,
        "source_index": index,
        "raw_source_utf8": String::from_utf8(raw).map_err(|error| format!("raw_utf8:{error}"))?,
        "columns": columns,
    }))
}

fn emit(field: &Field, array: &dyn Array, present: bool, row: usize) -> Result<Value, String> {
    if !present {
        return Ok(json!({"t": "absent", "present": false}));
    }
    if array.is_null(row) {
        return Ok(json!({"t": "null", "present": true}));
    }
    match array.data_type() {
        DataType::Utf8 => {
            let text = string_at(array, row)?;
            if field.metadata().get("oo.logical").map(String::as_str) == Some("json") {
                Ok(json!({"t": "json", "v": text, "present": true}))
            } else {
                Ok(json!({"t": "utf8", "v": text, "present": true}))
            }
        }
        DataType::Int64 => Ok(json!({"t": "i64", "v": int_at(array, row)?.to_string(), "present": true})),
        DataType::Float64 => Ok(json!({"t": "f64", "bits": float_at(array, row)?.to_bits().to_string(), "present": true})),
        DataType::Boolean => Ok(json!({"t": "bool", "v": bool_at(array, row)?, "present": true})),
        DataType::Binary => Ok(json!({"t": "bytes", "hex": hex(&binary_at(array, row)?), "present": true})),
        DataType::Timestamp(unit, _) => {
            let tag = match unit {
                TimeUnit::Microsecond => "ts_us",
                TimeUnit::Millisecond => "ts_ms",
                _ => return Err(format!("timestamp unit {unit:?}")),
            };
            Ok(json!({"t": tag, "v": timestamp_at(array, row, unit)?.to_string(), "present": true}))
        }
        DataType::List(item) => emit_list(item, array, row),
        DataType::Struct(_) => emit_struct(array, row),
        other => Err(format!("unsupported arrow type {other:?}")),
    }
}

fn emit_list(item: &Field, array: &dyn Array, row: usize) -> Result<Value, String> {
    let list = as_list_array(array);
    let values = list.value(row);
    let mut items = Vec::new();
    for index in 0..values.len() {
        if values.is_null(index) {
            items.push(json!({"t": "null", "present": true}));
        } else if item.data_type() == &DataType::Binary {
            let bytes = binary_at(values.as_ref(), index)?;
            match String::from_utf8(bytes) {
                Ok(text) => items.push(json!({"t": "utf8", "v": text, "present": true})),
                Err(error) => items.push(json!({"t": "bytes", "hex": hex(error.as_bytes()), "present": true})),
            }
        } else {
            items.push(emit(item, values.as_ref(), true, index)?);
        }
    }
    let kind = match item.data_type() {
        DataType::Utf8 => "list_utf8",
        DataType::Int64 => "list_i64",
        DataType::Binary => "list_binary",
        other => return Err(format!("list item {other:?}")),
    };
    Ok(json!({"t": kind, "v": items, "present": true}))
}

fn emit_struct(array: &dyn Array, row: usize) -> Result<Value, String> {
    let structure = array
        .as_any()
        .downcast_ref::<StructArray>()
        .ok_or("struct column")?;
    let mut children = Map::new();
    let mut index = 0;
    while index + 1 < structure.num_columns() {
        let field = structure.fields()[index].as_ref();
        let present_array = structure.column(index + 1);
        let present = bool_at(present_array.as_ref(), row)?;
        children.insert(field.name().clone(), emit(field, structure.column(index).as_ref(), present, row)?);
        index += 2;
    }
    Ok(json!({"t": "struct", "v": children, "present": true}))
}

fn string_at(array: &dyn Array, row: usize) -> Result<String, String> {
    let values = array.as_any().downcast_ref::<StringArray>().ok_or("utf8 column")?;
    if values.is_null(row) {
        return Err("unexpected null utf8".into());
    }
    Ok(values.value(row).to_string())
}

fn int_at(array: &dyn Array, row: usize) -> Result<i64, String> {
    let values = array.as_any().downcast_ref::<Int64Array>().ok_or("int64 column")?;
    if values.is_null(row) {
        return Err("unexpected null int64".into());
    }
    Ok(values.value(row))
}

fn float_at(array: &dyn Array, row: usize) -> Result<f64, String> {
    let values = array.as_any().downcast_ref::<Float64Array>().ok_or("float64 column")?;
    if values.is_null(row) {
        return Err("unexpected null float64".into());
    }
    Ok(values.value(row))
}

fn timestamp_at(array: &dyn Array, row: usize, unit: &TimeUnit) -> Result<i64, String> {
    match unit {
        TimeUnit::Microsecond => timestamp_value::<TimestampMicrosecondType>(array, row),
        TimeUnit::Millisecond => timestamp_value::<TimestampMillisecondType>(array, row),
        _ => Err(format!("timestamp unit {unit:?}")),
    }
}

fn timestamp_value<T>(array: &dyn Array, row: usize) -> Result<i64, String>
where
    T: arrow_array::ArrowPrimitiveType<Native = i64>,
{
    let values = array
        .as_any()
        .downcast_ref::<PrimitiveArray<T>>()
        .ok_or("timestamp column")?;
    if values.is_null(row) {
        return Err("unexpected null timestamp".into());
    }
    Ok(values.value(row))
}

fn bool_at(array: &dyn Array, row: usize) -> Result<bool, String> {
    let values = array.as_any().downcast_ref::<BooleanArray>().ok_or("bool column")?;
    if values.is_null(row) {
        return Err("unexpected null bool".into());
    }
    Ok(values.value(row))
}

fn binary_at(array: &dyn Array, row: usize) -> Result<Vec<u8>, String> {
    let values = array.as_any().downcast_ref::<BinaryArray>().ok_or("binary column")?;
    if values.is_null(row) {
        return Err("unexpected null binary".into());
    }
    Ok(values.value(row).to_vec())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}
