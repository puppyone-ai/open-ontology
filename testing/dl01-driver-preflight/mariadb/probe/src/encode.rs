use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::array::PrimitiveArray;
use arrow_array::types::TimestampMicrosecondType;
use arrow_array::{
    BinaryArray, Date32Array, Decimal128Array, Decimal256Array, Int16Array, Int32Array, Int64Array,
    Int8Array, RecordBatch, StringArray, UInt16Array, UInt32Array, UInt64Array, UInt8Array,
};
use arrow_buffer::i256;
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use arrow_ipc::MetadataVersion;
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use mysql_async::Value;

use crate::civil;
use crate::decimal::{self, Unscaled};
use crate::decide::{Kind, Plan};
use crate::fail::Fail;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Null,
    I64(i64),
    U64 { value: u64, from_uint: bool },
    Decimal(Unscaled),
    Utf8(String),
    Binary(Vec<u8>),
    Date(i32),
    Datetime(i64),
    Timestamp(i64),
    Interval(i64),
}

pub fn cell_from_value(plan: &Plan, value: &Value) -> Result<Cell, Fail> {
    if matches!(value, Value::Float(_) | Value::Double(_)) {
        return Err(Fail::new("unsafe_float", "wire float"));
    }
    match &plan.kind {
        Kind::Int { bits, unsigned: true } => unsigned_cell(*bits, value),
        Kind::Int { bits, unsigned: false } => signed_cell(*bits, value),
        Kind::Decimal { precision, scale } => decimal_cell(*precision, *scale, value),
        Kind::Utf8 { .. } => utf8_cell(value),
        Kind::Binary => binary_cell(value),
        Kind::Date => date_cell(value),
        Kind::Datetime => datetime_cell(value, false),
        Kind::Timestamp => datetime_cell(value, true),
        Kind::TimeInterval => time_cell(value),
    }
}

fn unsigned_cell(bits: u8, value: &Value) -> Result<Cell, Fail> {
    let (number, from_uint) = match value {
        Value::NULL => return Ok(Cell::Null),
        Value::UInt(number) => (*number, true),
        Value::Int(number) if *number >= 0 => (*number as u64, false),
        Value::Int(number) => return Err(Fail::new("unsigned_negative", number.to_string())),
        _ => return Err(Fail::new("value_shape", "unsigned")),
    };
    let max = match bits {
        8 => u64::from(u8::MAX),
        16 => u64::from(u16::MAX),
        32 => u64::from(u32::MAX),
        64 => u64::MAX,
        _ => return Err(Fail::new("unsupported_type", "unsigned width")),
    };
    if number > max {
        return Err(Fail::new("unsigned_range", number.to_string()));
    }
    if bits == 64 && number > i64::MAX as u64 && !from_uint {
        return Err(Fail::new("unsigned_narrowed", number.to_string()));
    }
    Ok(Cell::U64 { value: number, from_uint })
}

fn signed_cell(bits: u8, value: &Value) -> Result<Cell, Fail> {
    let number = match value {
        Value::NULL => return Ok(Cell::Null),
        Value::Int(number) => *number,
        Value::UInt(number) => i64::try_from(*number).map_err(|_| Fail::new("signed_range", number.to_string()))?,
        _ => return Err(Fail::new("value_shape", "signed")),
    };
    let (min, max) = match bits {
        8 => (i64::from(i8::MIN), i64::from(i8::MAX)),
        16 => (i64::from(i16::MIN), i64::from(i16::MAX)),
        32 => (i64::from(i32::MIN), i64::from(i32::MAX)),
        64 => (i64::MIN, i64::MAX),
        _ => return Err(Fail::new("unsupported_type", "signed width")),
    };
    if number < min || number > max {
        return Err(Fail::new("signed_range", number.to_string()));
    }
    Ok(Cell::I64(number))
}

fn decimal_cell(precision: u8, scale: u8, value: &Value) -> Result<Cell, Fail> {
    match value {
        Value::NULL => Ok(Cell::Null),
        Value::Bytes(bytes) => {
            let text = std::str::from_utf8(bytes).map_err(|_| Fail::new("utf8", "decimal"))?;
            Ok(Cell::Decimal(decimal::parse_decimal(text.trim(), precision, scale)?))
        }
        _ => Err(Fail::new("value_shape", "decimal")),
    }
}

fn utf8_cell(value: &Value) -> Result<Cell, Fail> {
    match value {
        Value::NULL => Ok(Cell::Null),
        Value::Bytes(bytes) => {
            let text = std::str::from_utf8(bytes).map_err(|_| Fail::new("utf8", "text"))?;
            Ok(Cell::Utf8(text.to_string()))
        }
        _ => Err(Fail::new("value_shape", "text")),
    }
}

fn binary_cell(value: &Value) -> Result<Cell, Fail> {
    match value {
        Value::NULL => Ok(Cell::Null),
        Value::Bytes(bytes) => Ok(Cell::Binary(bytes.clone())),
        _ => Err(Fail::new("value_shape", "binary")),
    }
}

fn date_cell(value: &Value) -> Result<Cell, Fail> {
    match value {
        Value::NULL => Ok(Cell::Null),
        Value::Date(year, month, day, hour, minute, second, micro) => {
            if *year == 0 || *month == 0 || *day == 0 {
                return Err(Fail::new("zero_date", format!("{year:04}-{month:02}-{day:02}")));
            }
            if *hour != 0 || *minute != 0 || *second != 0 || *micro != 0 {
                return Err(Fail::new("value_shape", "date has a clock time"));
            }
            Ok(Cell::Date(civil::days_from_civil(i32::from(*year), u32::from(*month), u32::from(*day))?))
        }
        _ => Err(Fail::new("value_shape", "date")),
    }
}

fn datetime_cell(value: &Value, utc: bool) -> Result<Cell, Fail> {
    match value {
        Value::NULL => Ok(Cell::Null),
        Value::Date(year, month, day, hour, minute, second, micro) => {
            if *year == 0 || *month == 0 || *day == 0 {
                return Err(Fail::new("zero_date", format!("{year:04}-{month:02}-{day:02}")));
            }
            let micros = civil::timestamp_us(
                i32::from(*year),
                u32::from(*month),
                u32::from(*day),
                u32::from(*hour),
                u32::from(*minute),
                u32::from(*second),
                *micro,
            )?;
            if utc {
                Ok(Cell::Timestamp(micros))
            } else {
                Ok(Cell::Datetime(micros))
            }
        }
        _ => Err(Fail::new("value_shape", "datetime")),
    }
}

fn time_cell(value: &Value) -> Result<Cell, Fail> {
    match value {
        Value::NULL => Ok(Cell::Null),
        Value::Time(negative, days, hours, minutes, seconds, micros) => Ok(Cell::Interval(civil::interval_us(
            *negative,
            *days,
            u32::from(*hours),
            u32::from(*minutes),
            u32::from(*seconds),
            *micros,
        )?)),
        _ => Err(Fail::new("value_shape", "time")),
    }
}

pub struct ColumnOut {
    pub name: String,
    pub nullable: bool,
    pub plan: Plan,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug)]
pub struct EncodedTable {
    pub rows: usize,
    pub batches: usize,
    pub bytes: Vec<u8>,
}

pub fn write_ipc(
    columns: &[ColumnOut],
    rows: &[Vec<Cell>],
    schema_meta: HashMap<String, String>,
    batch_rows: usize,
) -> Result<EncodedTable, Fail> {
    if batch_rows == 0 {
        return Err(Fail::new("batch_rows", "0"));
    }
    for (index, row) in rows.iter().enumerate() {
        if row.len() != columns.len() {
            return Err(Fail::new("row_width", format!("row {index}")));
        }
    }
    require_wide_unsigned(columns, rows)?;
    let fields = columns
        .iter()
        .map(|column| {
            let field = Field::new(column.name.clone(), arrow_type(&column.plan)?, column.nullable);
            Ok(field.with_metadata(column.metadata.clone()))
        })
        .collect::<Result<Vec<_>, Fail>>()?;
    let schema = Arc::new(Schema::new(fields).with_metadata(schema_meta));
    let spans = batch_spans(rows.len(), batch_rows);
    let mut buffer = Vec::new();
    {
        let options = IpcWriteOptions::try_new(8, false, MetadataVersion::V5)
            .map_err(|err| Fail::new("ipc", err.to_string()))?;
        let mut writer = StreamWriter::try_new_with_options(&mut buffer, schema.as_ref(), options)
            .map_err(|err| Fail::new("ipc", err.to_string()))?;
        for (start, end) in spans {
            let arrays = columns
                .iter()
                .enumerate()
                .map(|(index, column)| {
                    let cells: Vec<&Cell> = rows[start..end].iter().map(|row| &row[index]).collect();
                    column_array(column, &cells)
                })
                .collect::<Result<Vec<_>, Fail>>()?;
            let batch = RecordBatch::try_new(Arc::clone(&schema), arrays)
                .map_err(|err| Fail::new("ipc", err.to_string()))?;
            writer.write(&batch).map_err(|err| Fail::new("ipc", err.to_string()))?;
        }
        writer.finish().map_err(|err| Fail::new("ipc", err.to_string()))?;
    }
    Ok(EncodedTable {
        rows: rows.len(),
        batches: batch_spans(rows.len(), batch_rows).len(),
        bytes: buffer,
    })
}

fn require_wide_unsigned(columns: &[ColumnOut], rows: &[Vec<Cell>]) -> Result<(), Fail> {
    // mysql_common returns Int for an unsigned BIGINT that fits in i64, and
    // UInt only above that. A sibling column that carried u64::MAX proves the
    // uint64 path. A stream with no such value still fails.
    let mut saw_wide = false;
    let mut pending: Option<&str> = None;
    for (index, column) in columns.iter().enumerate() {
        let Kind::Int { bits: 64, unsigned: true } = column.plan.kind else {
            continue;
        };
        let mut wide = false;
        for row in rows {
            match row[index] {
                Cell::Null => {}
                Cell::U64 { value, from_uint: true } if value > i64::MAX as u64 => wide = true,
                Cell::U64 { value, .. } if value <= i64::MAX as u64 => {}
                _ => {
                    return Err(Fail::new(
                        "unsigned_narrowed",
                        format!("column.{} is not a native uint64 cell", column.name),
                    ));
                }
            }
        }
        if wide {
            saw_wide = true;
        } else if pending.is_none() {
            pending = Some(column.name.as_str());
        }
    }
    if !saw_wide {
        if let Some(name) = pending {
            return Err(Fail::new(
                "unsigned_narrowed",
                format!("column.{name} has no u64 value above i64::MAX"),
            ));
        }
    }
    Ok(())
}

fn batch_spans(len: usize, batch_rows: usize) -> Vec<(usize, usize)> {
    if len == 0 {
        return vec![(0, 0)];
    }
    let mut spans = Vec::new();
    let mut start = 0;
    while start < len {
        let end = (start + batch_rows).min(len);
        spans.push((start, end));
        start = end;
    }
    spans
}

fn decimal_scale(scale: u8) -> Result<i8, Fail> {
    i8::try_from(scale).map_err(|_| Fail::new("unsupported_precision", "scale"))
}

fn arrow_type(plan: &Plan) -> Result<DataType, Fail> {
    Ok(match &plan.kind {
        Kind::Int { bits: 8, unsigned: false } => DataType::Int8,
        Kind::Int { bits: 8, unsigned: true } => DataType::UInt8,
        Kind::Int { bits: 16, unsigned: false } => DataType::Int16,
        Kind::Int { bits: 16, unsigned: true } => DataType::UInt16,
        Kind::Int { bits: 32, unsigned: false } => DataType::Int32,
        Kind::Int { bits: 32, unsigned: true } => DataType::UInt32,
        Kind::Int { bits: 64, unsigned: false } => DataType::Int64,
        Kind::Int { bits: 64, unsigned: true } => DataType::UInt64,
        Kind::Int { .. } => return Err(Fail::new("unsupported_type", "int width")),
        Kind::Decimal { precision, scale } if *precision <= 38 => {
            DataType::Decimal128(*precision, decimal_scale(*scale)?)
        }
        Kind::Decimal { precision, scale } => DataType::Decimal256(*precision, decimal_scale(*scale)?),
        Kind::Utf8 { .. } => DataType::Utf8,
        Kind::Binary => DataType::Binary,
        Kind::Date => DataType::Date32,
        Kind::Datetime => DataType::Timestamp(TimeUnit::Microsecond, None),
        Kind::Timestamp => DataType::Timestamp(TimeUnit::Microsecond, Some("+00:00".into())),
        Kind::TimeInterval => DataType::Int64,
    })
}

fn column_array(column: &ColumnOut, cells: &[&Cell]) -> Result<arrow_array::ArrayRef, Fail> {
    for cell in cells {
        if !column.nullable && matches!(cell, Cell::Null) {
            return Err(Fail::new("null_violation", column.name.clone()));
        }
    }
    match &column.plan.kind {
        Kind::Int { bits, unsigned: true } => unsigned_array(*bits, cells),
        Kind::Int { bits, unsigned: false } => signed_array(*bits, cells),
        Kind::Decimal { precision, scale } => decimal_array(*precision, *scale, cells),
        Kind::Utf8 { .. } => {
            let values = cells
                .iter()
                .map(|cell| match cell {
                    Cell::Null => Ok(None),
                    Cell::Utf8(text) => Ok(Some(text.as_str())),
                    _ => Err(Fail::new("value_shape", "utf8")),
                })
                .collect::<Result<Vec<_>, Fail>>()?;
            Ok(Arc::new(StringArray::from(values)))
        }
        Kind::Binary => {
            let owned: Vec<Option<Vec<u8>>> = cells
                .iter()
                .map(|cell| match cell {
                    Cell::Null => Ok(None),
                    Cell::Binary(bytes) => Ok(Some(bytes.clone())),
                    _ => Err(Fail::new("value_shape", "binary")),
                })
                .collect::<Result<_, Fail>>()?;
            let borrowed: Vec<Option<&[u8]>> = owned.iter().map(|item| item.as_deref()).collect();
            Ok(Arc::new(BinaryArray::from(borrowed)))
        }
        Kind::Date => {
            let values = cells
                .iter()
                .map(|cell| match cell {
                    Cell::Null => Ok(None),
                    Cell::Date(days) => Ok(Some(*days)),
                    _ => Err(Fail::new("value_shape", "date")),
                })
                .collect::<Result<Vec<_>, Fail>>()?;
            Ok(Arc::new(Date32Array::from(values)))
        }
        Kind::Datetime => timestamp_array(cells, false),
        Kind::Timestamp => timestamp_array(cells, true),
        Kind::TimeInterval => {
            let values = cells
                .iter()
                .map(|cell| match cell {
                    Cell::Null => Ok(None),
                    Cell::Interval(micros) => Ok(Some(*micros)),
                    _ => Err(Fail::new("value_shape", "interval")),
                })
                .collect::<Result<Vec<_>, Fail>>()?;
            Ok(Arc::new(Int64Array::from(values)))
        }
    }
}

fn unsigned_array(bits: u8, cells: &[&Cell]) -> Result<arrow_array::ArrayRef, Fail> {
    let values = cells
        .iter()
        .map(|cell| match cell {
            Cell::Null => Ok(None),
            Cell::U64 { value, .. } => Ok(Some(*value)),
            _ => Err(Fail::new("value_shape", "unsigned")),
        })
        .collect::<Result<Vec<Option<u64>>, Fail>>()?;
    let array: arrow_array::ArrayRef = match bits {
        8 => Arc::new(UInt8Array::from(narrow_unsigned::<u8>(values)?)),
        16 => Arc::new(UInt16Array::from(narrow_unsigned::<u16>(values)?)),
        32 => Arc::new(UInt32Array::from(narrow_unsigned::<u32>(values)?)),
        64 => Arc::new(UInt64Array::from(values)),
        _ => return Err(Fail::new("unsupported_type", "unsigned width")),
    };
    Ok(array)
}

fn narrow_unsigned<T>(values: Vec<Option<u64>>) -> Result<Vec<Option<T>>, Fail>
where
    T: TryFrom<u64>,
{
    values
        .into_iter()
        .map(|item| match item {
            None => Ok(None),
            Some(value) => T::try_from(value).map(Some).map_err(|_| Fail::new("unsigned_range", value.to_string())),
        })
        .collect()
}

fn signed_array(bits: u8, cells: &[&Cell]) -> Result<arrow_array::ArrayRef, Fail> {
    let values = cells
        .iter()
        .map(|cell| match cell {
            Cell::Null => Ok(None),
            Cell::I64(value) => Ok(Some(*value)),
            _ => Err(Fail::new("value_shape", "signed")),
        })
        .collect::<Result<Vec<Option<i64>>, Fail>>()?;
    let array: arrow_array::ArrayRef = match bits {
        8 => Arc::new(Int8Array::from(narrow_signed::<i8>(values)?)),
        16 => Arc::new(Int16Array::from(narrow_signed::<i16>(values)?)),
        32 => Arc::new(Int32Array::from(narrow_signed::<i32>(values)?)),
        64 => Arc::new(Int64Array::from(values)),
        _ => return Err(Fail::new("unsupported_type", "signed width")),
    };
    Ok(array)
}

fn narrow_signed<T>(values: Vec<Option<i64>>) -> Result<Vec<Option<T>>, Fail>
where
    T: TryFrom<i64>,
{
    values
        .into_iter()
        .map(|item| match item {
            None => Ok(None),
            Some(value) => T::try_from(value).map(Some).map_err(|_| Fail::new("signed_range", value.to_string())),
        })
        .collect()
}

fn decimal_array(precision: u8, scale: u8, cells: &[&Cell]) -> Result<arrow_array::ArrayRef, Fail> {
    let scale_i = i8::try_from(scale).map_err(|_| Fail::new("unsupported_precision", "scale"))?;
    if precision <= 38 {
        let values = cells
            .iter()
            .map(|cell| match cell {
                Cell::Null => Ok(None),
                Cell::Decimal(Unscaled::I128(value)) => Ok(Some(*value)),
                Cell::Decimal(Unscaled::I256(_)) => Err(Fail::new("decimal_precision", "expected i128")),
                _ => Err(Fail::new("value_shape", "decimal128")),
            })
            .collect::<Result<Vec<_>, Fail>>()?;
        let array = Decimal128Array::from(values)
            .with_precision_and_scale(precision, scale_i)
            .map_err(|err| Fail::new("decimal_precision", err.to_string()))?;
        return Ok(Arc::new(array));
    }
    let values = cells
        .iter()
        .map(|cell| match cell {
            Cell::Null => Ok(None),
            Cell::Decimal(value) => Ok(Some(i256::from_le_bytes(decimal::i256_le(value)))),
            _ => Err(Fail::new("value_shape", "decimal256")),
        })
        .collect::<Result<Vec<_>, Fail>>()?;
    let array = Decimal256Array::from(values)
        .with_precision_and_scale(precision, scale_i)
        .map_err(|err| Fail::new("decimal_precision", err.to_string()))?;
    Ok(Arc::new(array))
}

fn timestamp_array(cells: &[&Cell], utc: bool) -> Result<arrow_array::ArrayRef, Fail> {
    let values = cells
        .iter()
        .map(|cell| match (utc, cell) {
            (_, Cell::Null) => Ok(None),
            (false, Cell::Datetime(value)) => Ok(Some(*value)),
            (true, Cell::Timestamp(value)) => Ok(Some(*value)),
            _ => Err(Fail::new("value_shape", "timestamp")),
        })
        .collect::<Result<Vec<_>, Fail>>()?;
    let array = PrimitiveArray::<TimestampMicrosecondType>::from(values);
    if utc {
        Ok(Arc::new(array.with_timezone("+00:00")))
    } else {
        Ok(Arc::new(array))
    }
}
