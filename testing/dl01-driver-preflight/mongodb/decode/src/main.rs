//! Independent Arrow IPC decoder. This binary does not link the MongoDB driver.

use arrow_array::Array;
use arrow_array::RecordBatch;
use arrow_array::cast::AsArray;
use arrow_array::types::Decimal128Type;
use arrow_array::types::Float64Type;
use arrow_array::types::Int32Type;
use arrow_array::types::Int64Type;
use arrow_array::types::TimestampMillisecondType;
use arrow_array::types::UInt8Type;
use arrow_ipc::reader::FileReader;
use arrow_schema::DataType;
use arrow_schema::TimeUnit;
use arrow_schema::UnionMode;
use serde_json::Value;
use serde_json::json;
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("oo-mongodb-arrow-decode: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut arrow_path = None;
    let mut out_path = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--arrow" => arrow_path = args.next(),
            "--out" => out_path = args.next(),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let arrow_path = arrow_path.ok_or("missing --arrow")?;
    let out_path = out_path.ok_or("missing --out")?;
    let file = match fs::File::open(&arrow_path) {
        Ok(file) => file,
        Err(err) => return fail(&out_path, format!("open: {err}")),
    };
    let reader = match FileReader::try_new(file, None) {
        Ok(reader) => reader,
        Err(err) => return fail(&out_path, format!("ipc: {err}")),
    };
    let schema = reader.schema();
    let schema_rows = schema_json(schema.as_ref());
    let batches = match reader.collect::<Result<Vec<_>, _>>() {
        Ok(batches) => batches,
        Err(err) => return fail_schema(&out_path, format!("batch: {err}"), &schema_rows),
    };
    if batches.len() != 1 {
        return fail_schema(&out_path, format!("batch count {}", batches.len()), &schema_rows);
    }
    let batch = &batches[0];
    let records = match records(batch) {
        Ok(records) => records,
        Err(err) => return fail_schema(&out_path, err, &schema_rows),
    };
    let body = json!({
        "ready": true,
        "decoder": "oo-mongodb-arrow-decode",
        "driver": "none",
        "rows": batch.num_rows(),
        "metadata": metadata_json(schema.metadata()),
        "schema": schema_rows,
        "records": records,
    });
    write_out(&out_path, &body)
}

fn schema_json(schema: &arrow_schema::Schema) -> Vec<Value> {
    schema
        .fields()
        .iter()
        .map(|field| {
            json!({
                "name": field.name(),
                "type": arrow_type(field.data_type()),
                "nullable": field.is_nullable(),
            })
        })
        .collect()
}

fn metadata_json(metadata: &arrow_schema::Metadata) -> Value {
    let mut object = serde_json::Map::new();
    for (key, value) in metadata.iter() {
        object.insert(key.clone(), Value::String(value.clone()));
    }
    Value::Object(object)
}

fn records(batch: &RecordBatch) -> Result<Vec<Value>, String> {
    let rows = batch.num_rows();
    let mut out = Vec::with_capacity(rows);
    for row in 0..rows {
        out.push(record(batch, row)?);
    }
    Ok(out)
}

fn record(batch: &RecordBatch, row: usize) -> Result<Value, String> {
    let oid = fixed(batch, "_id", row, 12)?;
    let uid = fixed(batch, "uid", row, 16)?;
    let (lane_tag, lane_text, lane_num) = lane(batch, row)?;
    let (pack_kind, pack_n) = pack(batch, row)?;
    let marks = marks(batch, row)?;
    let mut output = json!({
        "_id": hex(&oid),
        "code": utf8(batch, "code", row)?,
        "flag": i32_value(batch, "flag", row)?,
        "units": i64_text(batch, "units", row)?,
        "price": decimal_text(batch, "price", row)?,
        "created_ms": timestamp_ms(batch, "created", row)?,
        "uid_hex": hex(&uid),
        "blob_hex": hex(&binary(batch, "blob", row)?),
        "blob_subtype": Value::Null,
        "blob_subtype_stored": false,
        "blob_presence": u8_value(batch, "blob_presence", row)?,
        "weight": f64_value(batch, "weight", row)?,
        "warehouse": utf8(batch, "warehouse", row)?,
        "whCode": utf8(batch, "whCode", row)?,
        "lane_tag": lane_tag,
        "lane_text": lane_text,
        "lane_num": lane_num,
        "memo_presence": u8_value(batch, "memo_presence", row)?,
        "memo": utf8_nullable(batch, "memo", row)?,
        "marks_presence": u8_value(batch, "marks_presence", row)?,
        "marks": marks,
        "pack_presence": u8_value(batch, "pack_presence", row)?,
        "pack_kind": pack_kind,
        "pack_n": pack_n,
    });
    if batch.schema().index_of("doc").is_ok() {
        let text = utf8(batch, "doc", row)?;
        output["doc"] = serde_json::from_str(&text).map_err(|err| format!("doc JSON: {err}"))?;
    }
    Ok(output)
}

fn column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a dyn Array, String> {
    let index = batch.schema().index_of(name).map_err(|err| format!("{name}: {err}"))?;
    Ok(batch.column(index).as_ref())
}

fn fixed(batch: &RecordBatch, name: &str, row: usize, width: i32) -> Result<Vec<u8>, String> {
    let array = column(batch, name)?;
    match array.data_type() {
        DataType::FixedSizeBinary(actual) if *actual == width => {}
        other => return Err(format!("{name} {}", arrow_type(other))),
    }
    if array.is_null(row) {
        return Err(format!("{name} null"));
    }
    Ok(array.as_fixed_size_binary().value(row).to_vec())
}

fn utf8(batch: &RecordBatch, name: &str, row: usize) -> Result<String, String> {
    let array = column(batch, name)?;
    require_type(name, array, &DataType::Utf8)?;
    if array.is_null(row) {
        return Err(format!("{name} null"));
    }
    Ok(array.as_string::<i32>().value(row).to_string())
}

fn utf8_nullable(batch: &RecordBatch, name: &str, row: usize) -> Result<Value, String> {
    let array = column(batch, name)?;
    require_type(name, array, &DataType::Utf8)?;
    if array.is_null(row) {
        return Ok(Value::Null);
    }
    Ok(Value::String(array.as_string::<i32>().value(row).to_string()))
}

fn i32_value(batch: &RecordBatch, name: &str, row: usize) -> Result<i32, String> {
    let array = column(batch, name)?;
    require_type(name, array, &DataType::Int32)?;
    if array.is_null(row) {
        return Err(format!("{name} null"));
    }
    Ok(array.as_primitive::<Int32Type>().value(row))
}

fn i64_text(batch: &RecordBatch, name: &str, row: usize) -> Result<String, String> {
    let array = column(batch, name)?;
    require_type(name, array, &DataType::Int64)?;
    if array.is_null(row) {
        return Err(format!("{name} null"));
    }
    Ok(array.as_primitive::<Int64Type>().value(row).to_string())
}

fn decimal_text(batch: &RecordBatch, name: &str, row: usize) -> Result<String, String> {
    let array = column(batch, name)?;
    match array.data_type() {
        DataType::Decimal128(_, _) => {}
        other => return Err(format!("{name} {}", arrow_type(other))),
    }
    if array.is_null(row) {
        return Err(format!("{name} null"));
    }
    Ok(array.as_primitive::<Decimal128Type>().value(row).to_string())
}

fn timestamp_ms(batch: &RecordBatch, name: &str, row: usize) -> Result<i64, String> {
    let array = column(batch, name)?;
    match array.data_type() {
        DataType::Timestamp(TimeUnit::Millisecond, _) => {}
        other => return Err(format!("{name} {}", arrow_type(other))),
    }
    if array.is_null(row) {
        return Err(format!("{name} null"));
    }
    Ok(array.as_primitive::<TimestampMillisecondType>().value(row))
}

fn binary(batch: &RecordBatch, name: &str, row: usize) -> Result<Vec<u8>, String> {
    let array = column(batch, name)?;
    require_type(name, array, &DataType::Binary)?;
    if array.is_null(row) {
        return Err(format!("{name} null"));
    }
    Ok(array.as_binary::<i32>().value(row).to_vec())
}

fn u8_value(batch: &RecordBatch, name: &str, row: usize) -> Result<u8, String> {
    let array = column(batch, name)?;
    require_type(name, array, &DataType::UInt8)?;
    if array.is_null(row) {
        return Err(format!("{name} null"));
    }
    Ok(array.as_primitive::<UInt8Type>().value(row))
}

fn f64_value(batch: &RecordBatch, name: &str, row: usize) -> Result<f64, String> {
    let array = column(batch, name)?;
    require_type(name, array, &DataType::Float64)?;
    if array.is_null(row) {
        return Err(format!("{name} null"));
    }
    let value = array.as_primitive::<Float64Type>().value(row);
    if !value.is_finite() {
        return Err(format!("{name} non-finite"));
    }
    Ok(value)
}

fn lane(batch: &RecordBatch, row: usize) -> Result<(String, Value, Value), String> {
    let array = column(batch, "lane")?;
    let union = array.as_union_opt().ok_or_else(|| format!("lane {}", arrow_type(array.data_type())))?;
    let type_id = union.type_id(row);
    let offset = union.value_offset(row);
    let child = union.child(type_id);
    if type_id == 0 {
        require_type("lane.text", child.as_ref(), &DataType::Utf8)?;
        let text = child.as_string::<i32>().value(offset).to_string();
        return Ok(("utf8".to_string(), Value::String(text), Value::Null));
    }
    if type_id == 1 {
        require_type("lane.num", child.as_ref(), &DataType::Int64)?;
        let number = child.as_primitive::<Int64Type>().value(offset).to_string();
        return Ok(("int64".to_string(), Value::Null, Value::String(number)));
    }
    Err(format!("lane type id {type_id}"))
}

fn marks(batch: &RecordBatch, row: usize) -> Result<Value, String> {
    let array = column(batch, "marks")?;
    let Some(list) = array.as_list_opt::<i32>() else {
        return Err(format!("marks {}", arrow_type(array.data_type())));
    };
    if array.is_null(row) {
        return Ok(Value::Null);
    }
    let values = list.value(row);
    require_type("marks.item", values.as_ref(), &DataType::Utf8)?;
    let strings = values.as_string::<i32>();
    let mut items = Vec::with_capacity(strings.len());
    for value in strings.iter() {
        let Some(value) = value else {
            return Err(format!("marks null item row {row}"));
        };
        items.push(Value::String(value.to_string()));
    }
    Ok(Value::Array(items))
}

fn pack(batch: &RecordBatch, row: usize) -> Result<(Value, Value), String> {
    let array = column(batch, "pack")?;
    let Some(pack) = array.as_struct_opt() else {
        return Err(format!("pack {}", arrow_type(array.data_type())));
    };
    if array.is_null(row) {
        return Ok((Value::Null, Value::Null));
    }
    let kind = pack.column(0);
    let count = pack.column(1);
    require_type("pack.kind", kind.as_ref(), &DataType::Utf8)?;
    require_type("pack.n", count.as_ref(), &DataType::Int32)?;
    let kind_value = if kind.is_null(row) {
        Value::Null
    } else {
        Value::String(kind.as_string::<i32>().value(row).to_string())
    };
    let count_value = if count.is_null(row) {
        Value::Null
    } else {
        json!(count.as_primitive::<Int32Type>().value(row))
    };
    Ok((kind_value, count_value))
}

fn require_type(name: &str, array: &dyn Array, expect: &DataType) -> Result<(), String> {
    if array.data_type() == expect {
        Ok(())
    } else {
        Err(format!("{name} {}", arrow_type(array.data_type())))
    }
}

fn arrow_type(data_type: &DataType) -> String {
    match data_type {
        DataType::Utf8 => "utf8".to_string(),
        DataType::Int32 => "int32".to_string(),
        DataType::Int64 => "int64".to_string(),
        DataType::UInt8 => "uint8".to_string(),
        DataType::Float64 => "float64".to_string(),
        DataType::Binary => "binary".to_string(),
        DataType::FixedSizeBinary(width) => format!("fixedsizebinary({width})"),
        DataType::Decimal128(precision, scale) => format!("decimal128({precision},{scale})"),
        DataType::Timestamp(TimeUnit::Millisecond, zone) if zone.as_deref() == Some("UTC") => "timestamp(ms,utc)".to_string(),
        DataType::Timestamp(unit, zone) => format!("timestamp({unit:?},{zone:?})"),
        DataType::List(field) => format!("list({})", arrow_type(field.data_type())),
        DataType::Struct(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|field| format!("{}:{}", field.name(), arrow_type(field.data_type())))
                .collect();
            format!("struct({})", parts.join(","))
        }
        DataType::Union(fields, mode) => {
            let kind = match mode {
                UnionMode::Dense => "dense",
                UnionMode::Sparse => "sparse",
            };
            let mut parts: Vec<(i8, String)> = fields
                .iter()
                .map(|(id, field)| (id, format!("{id}:{}", arrow_type(field.data_type()))))
                .collect();
            parts.sort_by_key(|item| item.0);
            let rendered = parts.into_iter().map(|(_, text)| text).collect::<Vec<_>>().join(",");
            format!("union({kind},{rendered})")
        }
        other => format!("other:{other:?}"),
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn fail(out: &str, message: String) -> Result<(), String> {
    fail_schema(out, message, &[])
}

fn fail_schema(out: &str, message: String, schema: &[Value]) -> Result<(), String> {
    let body = json!({
        "ready": false,
        "error": &message,
        "decoder": "oo-mongodb-arrow-decode",
        "driver": "none",
        "schema": schema,
    });
    let _ = write_out(out, &body);
    Err(message)
}

fn write_out(path: &str, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
    fs::write(path, text + "\n").map_err(|err| err.to_string())
}
