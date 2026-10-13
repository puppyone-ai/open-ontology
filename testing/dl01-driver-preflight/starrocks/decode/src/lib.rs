#![forbid(unsafe_code)]

use std::fs::File;
use std::path::Path;

use arrow_array::{Array, Date32Array, Decimal128Array, Int64Array, StringArray, TimestampSecondArray};
use arrow_ipc::reader::StreamReader;
use arrow_schema::{DataType, TimeUnit};
use serde_json::{json, Map, Value};

pub fn decode_file(path: &Path) -> Result<Value, String> {
    let file = File::open(path).map_err(|err| err.to_string())?;
    let reader = StreamReader::try_new(file, None).map_err(|err| err.to_string())?;
    let schema = reader.schema();
    if schema.metadata().get("oo.source.protocol").map(String::as_str) != Some("oo.source.starrocks/v1") {
        return Err("ipc schema is not oo.source.starrocks/v1".to_string());
    }
    let mut fields = Vec::new();
    for field in schema.fields() {
        reject_unsafe(field.data_type())?;
        fields.push(json!({
            "name": field.name(),
            "nullable": field.is_nullable(),
            "type": type_name(field.data_type())?,
            "native_type": field.metadata().get("oo.starrocks.native_type"),
        }));
    }
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|err| err.to_string())?;
        for row_index in 0..batch.num_rows() {
            let mut object = Map::new();
            for (column_index, field) in schema.fields().iter().enumerate() {
                let column = batch.column(column_index);
                object.insert(field.name().clone(), cell_value(column.as_ref(), field.data_type(), row_index)?);
            }
            rows.push(Value::Object(object));
        }
    }
    let mut metadata = Map::new();
    for (key, value) in schema.metadata() {
        metadata.insert(key.clone(), Value::String(value.clone()));
    }
    Ok(json!({
        "protocol": "oo.source.starrocks.ipc-decode/v1",
        "schema_metadata": Value::Object(metadata),
        "fields": fields,
        "rows": rows,
    }))
}

fn reject_unsafe(data_type: &DataType) -> Result<(), String> {
    match data_type {
        DataType::Float16 | DataType::Float32 | DataType::Float64 => Err("unsafe float column".to_string()),
        DataType::Timestamp(_, Some(_)) => Err("timestamp carries an offset".to_string()),
        _ => Ok(()),
    }
}

fn type_name(data_type: &DataType) -> Result<String, String> {
    Ok(match data_type {
        DataType::Int64 => "int64".to_string(),
        DataType::Date32 => "date32".to_string(),
        DataType::Timestamp(TimeUnit::Second, None) => "timestamp_second_naive".to_string(),
        DataType::Decimal128(precision, scale) => format!("decimal128({precision},{scale})"),
        DataType::Utf8 => "utf8".to_string(),
        other => return Err(format!("unsupported arrow type {other}")),
    })
}

fn cell_value(array: &dyn Array, data_type: &DataType, index: usize) -> Result<Value, String> {
    if array.is_null(index) {
        return Ok(Value::Null);
    }
    match data_type {
        DataType::Int64 => {
            let column = array.as_any().downcast_ref::<Int64Array>().ok_or("int64")?;
            Ok(json!(column.value(index)))
        }
        DataType::Date32 => {
            let column = array.as_any().downcast_ref::<Date32Array>().ok_or("date32")?;
            Ok(Value::String(format_date(column.value(index))?))
        }
        DataType::Timestamp(TimeUnit::Second, None) => {
            let column = array.as_any().downcast_ref::<TimestampSecondArray>().ok_or("timestamp")?;
            if column.timezone().is_some() {
                return Err("timestamp carries an offset".to_string());
            }
            Ok(Value::String(format_naive(column.value(index))?))
        }
        DataType::Decimal128(_, scale) => {
            let column = array.as_any().downcast_ref::<Decimal128Array>().ok_or("decimal")?;
            Ok(Value::String(format_decimal(column.value(index), *scale)?))
        }
        DataType::Utf8 => {
            let column = array.as_any().downcast_ref::<StringArray>().ok_or("utf8")?;
            Ok(Value::String(column.value(index).to_string()))
        }
        other => Err(format!("unsupported arrow type {other}")),
    }
}

pub fn format_decimal(value: i128, scale: i8) -> Result<String, String> {
    if scale < 0 {
        return Err("negative scale".to_string());
    }
    let negative = value < 0;
    let digits = value.unsigned_abs().to_string();
    let scale = usize::try_from(scale).map_err(|_| "scale".to_string())?;
    let body = if scale == 0 {
        digits
    } else if digits.len() <= scale {
        let padded = format!("{digits:0>scale$}");
        format!("0.{padded}")
    } else {
        let split = digits.len() - scale;
        format!("{}.{}", &digits[..split], &digits[split..])
    };
    if negative { Ok(format!("-{body}")) } else { Ok(body) }
}

fn format_date(days: i32) -> Result<String, String> {
    let (year, month, day) = days_to_ymd(days)?;
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

fn format_naive(seconds: i64) -> Result<String, String> {
    let days = i32::try_from(seconds.div_euclid(86_400)).map_err(|_| "datetime range".to_string())?;
    let tod = seconds.rem_euclid(86_400);
    let date = format_date(days)?;
    Ok(format!("{date} {:02}:{:02}:{:02}", tod / 3600, (tod % 3600) / 60, tod % 60))
}

fn days_to_ymd(days: i32) -> Result<(i32, u32, u32), String> {
    let z = i64::from(days) + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = u64::try_from(z - era * 146097).map_err(|_| "date".to_string())?;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = i32::try_from(yoe as i64 + era * 400).map_err(|_| "year".to_string())?;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_part = (5 * doy + 2) / 153;
    let day = doy - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 { month_part + 3 } else { month_part - 9 };
    if month <= 2 {
        year += 1;
    }
    Ok((year, u32::try_from(month).map_err(|_| "month".to_string())?, u32::try_from(day).map_err(|_| "day".to_string())?))
}

#[cfg(test)]
mod tests {
    use super::format_decimal;

    #[test]
    fn decimal_text_keeps_scale() {
        assert_eq!(format_decimal(45_000, 4).unwrap(), "4.5000");
        assert_eq!(format_decimal(-3, 4).unwrap(), "-0.0003");
        assert_eq!(format_decimal(0, 4).unwrap(), "0.0000");
        assert_eq!(
            format_decimal(22_345_678_901_234_567_890_123_456_789_123_456_789, 9).unwrap(),
            "22345678901234567890123456789.123456789"
        );
    }
}
