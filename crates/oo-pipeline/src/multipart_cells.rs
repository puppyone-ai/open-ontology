use crate::native_arrow::{DecodedStream, NativeCell};
use serde_json::{Value, json};

pub fn text_cell(cell: &NativeCell) -> Result<Value, &'static str> {
    Ok(match cell {
        NativeCell::Null => Value::Null,
        NativeCell::Utf8(text) | NativeCell::Json(text) => json!(text),
        NativeCell::Int { value, .. } => json!(value.to_string()),
        NativeCell::UInt64(value) => json!(value.to_string()),
        NativeCell::UInt8(value) => json!(value.to_string()),
        NativeCell::Binary(value) => json!(format!(
            "\\x{}",
            value.iter().map(|b| format!("{b:02x}")).collect::<String>()
        )),
        NativeCell::Decimal {
            scale, coefficient, ..
        } => json!(
            oo_core::ExactDecimal::new(
                *coefficient < 0,
                coefficient.unsigned_abs().to_string(),
                i32::from(*scale)
            )
            .map_err(|_| "multipart_decimal")?
            .to_string()
        ),
        NativeCell::Decimal256 {
            scale, coefficient, ..
        } => {
            let (negative, digits) = crate::native_arrow::i256_digits(coefficient);
            json!(
                oo_core::ExactDecimal::new(negative, digits, i32::from(*scale))
                    .map_err(|_| "multipart_decimal")?
                    .to_string()
            )
        }
        NativeCell::TimestampMicros(value) => json!(timestamp(*value)?),
        NativeCell::TimestampMicrosUtc { value, timezone } if timezone == "UTC" => {
            json!(format!("{}+00:00", timestamp(*value)?))
        }
        NativeCell::Date32(days) => {
            let (year, month, day) = crate::native_arrow::civil_from_unix_days(i64::from(*days))
                .ok_or("multipart_date")?;
            oo_core::canonical::validate_date(year, month, day).map_err(|_| "multipart_date")?;
            json!(format!("{year:04}-{month:02}-{day:02}"))
        }
        NativeCell::Float64(bits) => json!(f64::from_bits(*bits).to_string()),
        NativeCell::Bool(value) => json!(if *value { "1" } else { "0" }),
        _ => return Err("multipart_cell_type"),
    })
}

fn timestamp(value: i64) -> Result<String, &'static str> {
    let (year, month, day, hour, minute, second, micros) =
        crate::native_arrow::civil_timestamp(value).ok_or("multipart_timestamp")?;
    oo_core::canonical::validate_date(year, month, day).map_err(|_| "multipart_timestamp")?;
    Ok(format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micros:06}"
    ))
}

pub fn keys(decoded: &DecodedStream, indexes: &[usize]) -> Result<Value, &'static str> {
    if indexes.is_empty() {
        return Err("multipart_keys");
    }
    (0..decoded.rows)
        .map(|row| {
            indexes
                .iter()
                .map(|index| {
                    let value = text_cell(
                        decoded
                            .columns
                            .get(*index)
                            .and_then(|column| column.get(row))
                            .ok_or("multipart_keys")?,
                    )?;
                    if value.is_null() {
                        return Err("multipart_keys");
                    }
                    Ok(value)
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_key_audit_formats_nonkey_temporal_numeric_and_float_cells() {
        let cells = [
            (
                NativeCell::Decimal {
                    precision: 32,
                    scale: 4,
                    coefficient: -1234500,
                },
                "-123.4500",
            ),
            (
                NativeCell::TimestampMicros(-1),
                "1969-12-31 23:59:59.999999",
            ),
            (
                NativeCell::TimestampMicrosUtc {
                    value: 0,
                    timezone: "UTC".into(),
                },
                "1970-01-01 00:00:00.000000+00:00",
            ),
            (NativeCell::Date32(0), "1970-01-01"),
            (NativeCell::Float64((-0.0f64).to_bits()), "-0"),
            (NativeCell::Bool(true), "1"),
            (
                NativeCell::Decimal256 {
                    precision: 65,
                    scale: 2,
                    coefficient: [0; 32],
                },
                "0.00",
            ),
        ];
        for (cell, expected) in cells {
            assert_eq!(text_cell(&cell).unwrap(), json!(expected));
        }
        assert!(
            text_cell(&NativeCell::TimestampMicrosUtc {
                value: 0,
                timezone: "other".into()
            })
            .is_err()
        );
    }
}
