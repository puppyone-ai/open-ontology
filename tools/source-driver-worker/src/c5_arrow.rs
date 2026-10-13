//! Arrow IPC for one C5 snapshot. schema_digest is not written here.

use std::io::Cursor;

use arrow_array::builder::Decimal128Builder;
use arrow_array::{Array, BinaryArray, Int16Array, Int8Array, RecordBatch, StringArray, TimestampMicrosecondArray};
use arrow_ipc::reader::StreamReader;
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use arrow_ipc::MetadataVersion;
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const ALIGNMENT: usize = 8;

pub fn encode_table(spec: &Value) -> Result<Vec<u8>, String> {
    let object = spec.as_object().ok_or("spec")?;
    if object.contains_key("schema_digest") || object.contains_key("admission_receipt") {
        return Err("hand_filled_digest".to_owned());
    }
    let columns = object.get("columns").and_then(Value::as_array).ok_or("columns")?;
    let rows = object.get("rows").and_then(Value::as_array).ok_or("rows")?;
    let mut fields = Vec::with_capacity(columns.len());
    let mut arrays: Vec<arrow_array::ArrayRef> = Vec::with_capacity(columns.len());
    for column in columns {
        let column = column.as_object().ok_or("column")?;
        let name = column.get("name").and_then(Value::as_str).ok_or("column")?;
        let kind = column.get("type").and_then(Value::as_str).ok_or("column")?;
        let nullable = column.get("nullable").and_then(Value::as_bool).ok_or("column")?;
        let (field, array) = column_array(name, kind, nullable, column, rows)?;
        fields.push(field);
        arrays.push(array);
    }
    let schema = Schema::new(fields);
    let batch = RecordBatch::try_new(std::sync::Arc::new(schema.clone()), arrays).map_err(|err| err.to_string())?;
    let options = IpcWriteOptions::try_new(ALIGNMENT, false, MetadataVersion::V5).map_err(|err| err.to_string())?;
    let mut writer = StreamWriter::try_new_with_options(Vec::<u8>::new(), &schema, options).map_err(|err| err.to_string())?;
    writer.write(&batch).map_err(|err| err.to_string())?;
    writer.finish().map_err(|err| err.to_string())?;
    writer.into_inner().map_err(|err| err.to_string())
}

pub fn arrow_sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn decode_table(bytes: &[u8]) -> Result<Value, String> {
    let mut reader = StreamReader::try_new(Cursor::new(bytes), None).map_err(|err| err.to_string())?;
    let batch = reader
        .next()
        .ok_or("empty")?
        .map_err(|err| err.to_string())?;
    if reader.next().is_some() {
        return Err("extra_batch".to_owned());
    }
    let mut columns = Vec::new();
    let mut rows = vec![Map::new(); batch.num_rows()];
    for (index, field) in batch.schema().fields().iter().enumerate() {
        let kind = kind_name(field.data_type())?;
        columns.push(serde_json::json!({
            "name": field.name(),
            "type": kind,
            "nullable": field.is_nullable(),
            "precision": precision_of(field.data_type()),
            "scale": scale_of(field.data_type()),
        }));
        fill_column(&mut rows, field.name(), field.data_type(), batch.column(index))?;
    }
    Ok(serde_json::json!({
        "columns": columns,
        "rows": rows,
    }))
}

fn column_array(
    name: &str,
    kind: &str,
    nullable: bool,
    column: &serde_json::Map<String, Value>,
    rows: &[Value],
) -> Result<(Field, arrow_array::ArrayRef), String> {
    match kind {
        "int16" => {
            let mut values = Vec::with_capacity(rows.len());
            for row in rows {
                values.push(match optional_i64(row, name, nullable)? {
                    None => None,
                    Some(value) => Some(i16_from(value)?),
                });
            }
            Ok((Field::new(name, DataType::Int16, nullable), std::sync::Arc::new(Int16Array::from(values))))
        }
        "int8" => {
            let mut values = Vec::with_capacity(rows.len());
            for row in rows {
                values.push(match optional_i64(row, name, nullable)? {
                    None => None,
                    Some(value) => Some(i8_from(value)?),
                });
            }
            Ok((Field::new(name, DataType::Int8, nullable), std::sync::Arc::new(Int8Array::from(values))))
        }
        "utf8" => {
            let mut values = Vec::with_capacity(rows.len());
            for row in rows {
                values.push(optional_string(row, name, nullable)?);
            }
            Ok((Field::new(name, DataType::Utf8, nullable), std::sync::Arc::new(StringArray::from(values))))
        }
        "binary" => {
            let mut values: Vec<Option<Vec<u8>>> = Vec::with_capacity(rows.len());
            for row in rows {
                values.push(match optional_string(row, name, nullable)? {
                    None => None,
                    Some(text) => Some(decode_hex(&text)?),
                });
            }
            let refs: Vec<Option<&[u8]>> = values.iter().map(|item| item.as_deref()).collect();
            Ok((Field::new(name, DataType::Binary, nullable), std::sync::Arc::new(BinaryArray::from(refs))))
        }
        "decimal128" => {
            let precision = column.get("precision").and_then(Value::as_u64).ok_or("precision")? as u8;
            let scale = column.get("scale").and_then(Value::as_i64).ok_or("scale")? as i8;
            let mut builder = Decimal128Builder::with_capacity(rows.len())
                .with_precision_and_scale(precision, scale)
                .map_err(|err| err.to_string())?;
            for row in rows {
                match optional_string(row, name, nullable)? {
                    None => builder.append_null(),
                    Some(text) => builder.append_value(parse_decimal(&text, precision, scale)?),
                }
            }
            let array = builder.finish();
            Ok((
                Field::new(name, DataType::Decimal128(precision, scale), nullable),
                std::sync::Arc::new(array),
            ))
        }
        "timestamp_us" => {
            let mut values = Vec::with_capacity(rows.len());
            for row in rows {
                values.push(match optional_string(row, name, nullable)? {
                    None => None,
                    Some(text) => Some(parse_timestamp_us(&text)?),
                });
            }
            Ok((
                Field::new(name, DataType::Timestamp(TimeUnit::Microsecond, None), nullable),
                std::sync::Arc::new(TimestampMicrosecondArray::from(values)),
            ))
        }
        _ => Err("type".to_owned()),
    }
}

fn fill_column(rows: &mut [Map<String, Value>], name: &str, data_type: &DataType, array: &dyn Array) -> Result<(), String> {
    match data_type {
        DataType::Int16 => {
            let values = array.as_any().downcast_ref::<Int16Array>().ok_or("cast")?;
            for (row, target) in rows.iter_mut().enumerate() {
                target.insert(name.to_owned(), if values.is_null(row) { Value::Null } else { Value::from(i64::from(values.value(row))) });
            }
        }
        DataType::Int8 => {
            let values = array.as_any().downcast_ref::<Int8Array>().ok_or("cast")?;
            for (row, target) in rows.iter_mut().enumerate() {
                target.insert(name.to_owned(), if values.is_null(row) { Value::Null } else { Value::from(i64::from(values.value(row))) });
            }
        }
        DataType::Utf8 => {
            let values = array.as_any().downcast_ref::<StringArray>().ok_or("cast")?;
            for (row, target) in rows.iter_mut().enumerate() {
                target.insert(
                    name.to_owned(),
                    if values.is_null(row) { Value::Null } else { Value::from(values.value(row)) },
                );
            }
        }
        DataType::Binary => {
            let values = array.as_any().downcast_ref::<BinaryArray>().ok_or("cast")?;
            for (row, target) in rows.iter_mut().enumerate() {
                target.insert(
                    name.to_owned(),
                    if values.is_null(row) { Value::Null } else { Value::from(encode_hex(values.value(row))) },
                );
            }
        }
        DataType::Decimal128(_, scale) => {
            let values = array.as_any().downcast_ref::<arrow_array::Decimal128Array>().ok_or("cast")?;
            for (row, target) in rows.iter_mut().enumerate() {
                target.insert(
                    name.to_owned(),
                    if values.is_null(row) { Value::Null } else { Value::from(format_decimal(values.value(row), *scale)) },
                );
            }
        }
        DataType::Timestamp(TimeUnit::Microsecond, None) => {
            let values = array.as_any().downcast_ref::<TimestampMicrosecondArray>().ok_or("cast")?;
            for (row, target) in rows.iter_mut().enumerate() {
                target.insert(
                    name.to_owned(),
                    if values.is_null(row) { Value::Null } else { Value::from(format_timestamp_us(values.value(row))?) },
                );
            }
        }
        _ => return Err("type".to_owned()),
    }
    Ok(())
}

fn kind_name(data_type: &DataType) -> Result<&'static str, String> {
    Ok(match data_type {
        DataType::Int16 => "int16",
        DataType::Int8 => "int8",
        DataType::Utf8 => "utf8",
        DataType::Binary => "binary",
        DataType::Decimal128(_, _) => "decimal128",
        DataType::Timestamp(TimeUnit::Microsecond, None) => "timestamp_us",
        _ => return Err("type".to_owned()),
    })
}

fn precision_of(data_type: &DataType) -> Option<u8> {
    match data_type {
        DataType::Decimal128(precision, _) => Some(*precision),
        _ => None,
    }
}

fn scale_of(data_type: &DataType) -> Option<i8> {
    match data_type {
        DataType::Decimal128(_, scale) => Some(*scale),
        _ => None,
    }
}

fn optional_i64(row: &Value, name: &str, nullable: bool) -> Result<Option<i64>, String> {
    let value = row.get(name).ok_or("missing")?;
    if value.is_null() {
        if nullable {
            Ok(None)
        } else {
            Err("null".to_owned())
        }
    } else {
        value.as_i64().map(Some).ok_or("int".to_owned())
    }
}

fn optional_string(row: &Value, name: &str, nullable: bool) -> Result<Option<String>, String> {
    let value = row.get(name).ok_or("missing")?;
    if value.is_null() {
        if nullable {
            Ok(None)
        } else {
            Err("null".to_owned())
        }
    } else {
        value.as_str().map(|text| Some(text.to_owned())).ok_or("text".to_owned())
    }
}

fn i16_from(value: i64) -> Result<i16, String> {
    i16::try_from(value).map_err(|_| "int16".to_owned())
}

fn i8_from(value: i64) -> Result<i8, String> {
    i8::try_from(value).map_err(|_| "int8".to_owned())
}

fn parse_decimal(text: &str, precision: u8, scale: i8) -> Result<i128, String> {
    if scale < 0 {
        return Err("scale".to_owned());
    }
    let negative = text.starts_with('-');
    let body = text.strip_prefix('-').unwrap_or(text);
    if body.is_empty() || body.chars().filter(|ch| *ch == '.').count() > 1 {
        return Err("decimal".to_owned());
    }
    let (whole, frac) = body.split_once('.').unwrap_or((body, ""));
    if whole.is_empty()
        || !whole.chars().all(|ch| ch.is_ascii_digit())
        || !frac.chars().all(|ch| ch.is_ascii_digit())
    {
        return Err("decimal".to_owned());
    }
    let scale_len = scale as usize;
    if frac.len() > scale_len && frac[scale_len..].chars().any(|ch| ch != '0') {
        return Err("decimal_scale".to_owned());
    }
    let mut frac_padded = frac.chars().take(scale_len).collect::<String>();
    while frac_padded.len() < scale_len {
        frac_padded.push('0');
    }
    let digits = format!("{whole}{frac_padded}");
    let trimmed = digits.trim_start_matches('0');
    let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
    if trimmed.len() > precision as usize {
        return Err("decimal_precision".to_owned());
    }
    let mut value: i128 = trimmed.parse().map_err(|_| "decimal".to_owned())?;
    if negative {
        value = -value;
    }
    Ok(value)
}

fn format_decimal(value: i128, scale: i8) -> String {
    let negative = value < 0;
    let digits = value.unsigned_abs().to_string();
    let scale_len = scale as usize;
    let body = if scale_len == 0 {
        digits
    } else if digits.len() <= scale_len {
        format!("0.{:0>width$}", digits, width = scale_len)
    } else {
        let split = digits.len() - scale_len;
        format!("{}.{}", &digits[..split], &digits[split..])
    };
    if negative { format!("-{body}") } else { body }
}

fn parse_timestamp_us(text: &str) -> Result<i64, String> {
    if text.len() != 19 || text.as_bytes().get(10) != Some(&b'T') {
        return Err("timestamp".to_owned());
    }
    let year: i32 = text[0..4].parse().map_err(|_| "timestamp")?;
    let month: u32 = text[5..7].parse().map_err(|_| "timestamp")?;
    let day: u32 = text[8..10].parse().map_err(|_| "timestamp")?;
    let hour: u32 = text[11..13].parse().map_err(|_| "timestamp")?;
    let minute: u32 = text[14..16].parse().map_err(|_| "timestamp")?;
    let second: u32 = text[17..19].parse().map_err(|_| "timestamp")?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err("timestamp".to_owned());
    }
    let days = civil_days(year, month, day);
    days.checked_mul(86_400)
        .and_then(|seconds| seconds.checked_add(i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second)))
        .and_then(|seconds| seconds.checked_mul(1_000_000))
        .ok_or("timestamp".to_owned())
}

fn format_timestamp_us(micros: i64) -> Result<String, String> {
    if micros.rem_euclid(1_000_000) != 0 {
        return Err("timestamp_fraction".to_owned());
    }
    let seconds = micros / 1_000_000;
    let days = seconds.div_euclid(86_400);
    let tod = seconds.rem_euclid(86_400) as u32;
    let (year, month, day) = civil_ymd(days);
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        tod / 3_600,
        (tod % 3_600) / 60,
        tod % 60
    ))
}

fn civil_days(year: i32, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = (year - era * 400) as u32;
    let month_prime = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    i64::from(era) * 146_097 + i64::from(doe) - 719_468
}

fn civil_ymd(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) || !text.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("hex".to_owned());
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).map_err(|_| "hex".to_owned()))
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_keeps_oracle_cell_text_and_refuses_a_hand_filled_digest() {
        let spec = serde_json::json!({
            "columns": [
                {"name": "ID", "type": "int16", "nullable": false},
                {"name": "N16", "type": "decimal128", "precision": 16, "scale": 6, "nullable": true},
                {"name": "N1", "type": "int8", "nullable": true},
                {"name": "D", "type": "timestamp_us", "nullable": true},
                {"name": "C6", "type": "utf8", "nullable": true},
                {"name": "V2", "type": "utf8", "nullable": true},
                {"name": "R", "type": "binary", "nullable": true}
            ],
            "rows": [
                {"ID": 41, "N16": "3141592653.589793", "N1": 0, "D": "2026-10-10T07:11:42", "C6": "ZQ    ", "V2": null, "R": "0001FF00"},
                {"ID": 42, "N16": "-0.000001", "N1": null, "D": null, "C6": null, "V2": "a b", "R": null}
            ]
        });
        let bytes = encode_table(&spec).expect("encode");
        assert!(!bytes.is_empty());
        let decoded = decode_table(&bytes).expect("decode");
        assert_eq!(decoded["rows"][0]["N16"], "3141592653.589793");
        assert_eq!(decoded["rows"][0]["C6"], "ZQ    ");
        assert!(decoded["rows"][0]["V2"].is_null());
        assert_eq!(decoded["rows"][0]["R"], "0001FF00");
        assert_eq!(decoded["rows"][1]["N16"], "-0.000001");
        assert_eq!(decoded["rows"][1]["V2"], "a b");
        assert_eq!(decoded["rows"][0]["D"], "2026-10-10T07:11:42");
        let mut forged = spec.clone();
        forged["schema_digest"] = serde_json::json!("ab".repeat(32));
        assert_eq!(encode_table(&forged).unwrap_err(), "hand_filled_digest");
        assert_eq!(arrow_sha256(&bytes).len(), 64);
    }

    #[test]
    fn unix_epoch_is_zero_days() {
        assert_eq!(parse_timestamp_us("1970-01-01T00:00:00").expect("epoch"), 0);
    }
}
