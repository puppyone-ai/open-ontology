use mysql_async::Value;

use crate::civil;
use crate::decimal;
use crate::fail::Fail;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Int64,
    Utf8,
    Date,
    Decimal { precision: u8, scale: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Null,
    Int(i64),
    Text(String),
    Date(i32),
    Decimal(i128),
}

pub fn from_value(value: &Value, kind: Kind) -> Result<Cell, Fail> {
    if matches!(value, Value::Float(_) | Value::Double(_)) {
        return Err(Fail::new("float_rejected", "text protocol float is not a decimal"));
    }
    if matches!(value, Value::NULL) {
        return Ok(Cell::Null);
    }
    match kind {
        Kind::Int64 => Ok(Cell::Int(as_i64(value)?)),
        Kind::Utf8 => Ok(Cell::Text(as_text(value)?)),
        Kind::Date => match value {
            Value::Date(year, month, day, hour, minute, second, micros) => {
                if *hour != 0 || *minute != 0 || *second != 0 || *micros != 0 {
                    return Err(Fail::new("date", "time component"));
                }
                Ok(Cell::Date(civil::unix_days(i32::from(*year), u32::from(*month), u32::from(*day))?))
            }
            Value::Bytes(_) | Value::Int(_) | Value::UInt(_) => Ok(Cell::Date(civil::parse_date(&as_text(value)?)?)),
            _ => Err(Fail::new("date", "value")),
        },
        Kind::Decimal { precision, scale } => {
            let text = match value {
                Value::Bytes(_) | Value::Int(_) | Value::UInt(_) => as_text(value)?,
                _ => return Err(Fail::new("decimal_text", "value")),
            };
            Ok(Cell::Decimal(decimal::parse_decimal(&text, precision, scale)?))
        }
    }
}

pub fn as_text(value: &Value) -> Result<String, Fail> {
    match value {
        Value::NULL => Err(Fail::new("null", "text")),
        Value::Bytes(bytes) => {
            String::from_utf8(bytes.clone()).map_err(|_| Fail::new("utf8", "value"))
        }
        Value::Int(number) => Ok(number.to_string()),
        Value::UInt(number) => Ok(number.to_string()),
        Value::Date(year, month, day, hour, minute, second, _) => Ok(format!(
            "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"
        )),
        Value::Time(negative, days, hour, minute, second, _) => Ok(format!(
            "time neg={negative} days={days} {hour:02}:{minute:02}:{second:02}"
        )),
        Value::Float(_) | Value::Double(_) => Err(Fail::new("float_rejected", "text")),
    }
}

pub fn as_optional_text(value: &Value) -> Result<Option<String>, Fail> {
    if matches!(value, Value::NULL) {
        Ok(None)
    } else {
        Ok(Some(as_text(value)?))
    }
}

fn as_i64(value: &Value) -> Result<i64, Fail> {
    match value {
        Value::Int(number) => Ok(*number),
        Value::UInt(number) => i64::try_from(*number).map_err(|_| Fail::new("signed_range", number.to_string())),
        Value::Bytes(bytes) => {
            let text = std::str::from_utf8(bytes).map_err(|_| Fail::new("utf8", "int"))?;
            text.trim()
                .parse::<i64>()
                .map_err(|_| Fail::new("int", text.to_string()))
        }
        _ => Err(Fail::new("int", "value")),
    }
}
