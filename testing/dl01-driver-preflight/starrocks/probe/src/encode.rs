use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{Date32Array, Decimal128Array, Int64Array, RecordBatch, StringArray, TimestampSecondArray};
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use arrow_ipc::MetadataVersion;
use arrow_schema::{DataType, Field, Schema};

use crate::fail::Fail;
use crate::plan::ArrowKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Null,
    I64(i64),
    Decimal(i128),
    Date(i32),
    NaiveSecond(i64),
    Text(String),
}

pub struct ColumnOut {
    pub name: String,
    pub nullable: bool,
    pub kind: ArrowKind,
    pub native_type: String,
}

pub fn write_ipc(
    columns: &[ColumnOut],
    rows: &[Vec<Cell>],
    schema_meta: HashMap<String, String>,
    batch_rows: usize,
) -> Result<Vec<u8>, Fail> {
    if batch_rows == 0 {
        return Err(Fail::new("batch_rows", "0"));
    }
    for (index, row) in rows.iter().enumerate() {
        if row.len() != columns.len() {
            return Err(Fail::new("row_width", format!("row {index}")));
        }
    }
    let fields = columns
        .iter()
        .map(|column| {
            let field = Field::new(column.name.clone(), arrow_type(column.kind)?, column.nullable);
            let mut metadata = HashMap::new();
            metadata.insert("oo.starrocks.native_type".to_string(), column.native_type.clone());
            Ok(field.with_metadata(metadata))
        })
        .collect::<Result<Vec<_>, Fail>>()?;
    let schema = Arc::new(Schema::new(fields).with_metadata(schema_meta));
    let mut buffer = Vec::new();
    let options = IpcWriteOptions::try_new(8, false, MetadataVersion::V5)
        .map_err(|err| Fail::new("ipc", err.to_string()))?;
    let mut writer = StreamWriter::try_new_with_options(&mut buffer, schema.as_ref(), options)
        .map_err(|err| Fail::new("ipc", err.to_string()))?;
    if rows.is_empty() {
        return Err(Fail::new("empty_batch", "refusing an empty success batch"));
    }
    let mut start = 0;
    while start < rows.len() {
        let end = (start + batch_rows).min(rows.len());
        let arrays = columns
            .iter()
            .enumerate()
            .map(|(index, column)| column_array(column, &rows[start..end], index))
            .collect::<Result<Vec<_>, Fail>>()?;
        let batch = RecordBatch::try_new(Arc::clone(&schema), arrays)
            .map_err(|err| Fail::new("ipc", err.to_string()))?;
        writer.write(&batch).map_err(|err| Fail::new("ipc", err.to_string()))?;
        start = end;
    }
    writer.finish().map_err(|err| Fail::new("ipc", err.to_string()))?;
    Ok(buffer)
}

fn arrow_type(kind: ArrowKind) -> Result<DataType, Fail> {
    Ok(match kind {
        ArrowKind::Int64 => DataType::Int64,
        ArrowKind::Decimal { precision, scale } => {
            DataType::Decimal128(precision, i8::try_from(scale).map_err(|_| Fail::new("decimal", "scale"))?)
        }
        ArrowKind::Date32 => DataType::Date32,
        ArrowKind::NaiveTimestampSecond => DataType::Timestamp(arrow_schema::TimeUnit::Second, None),
        ArrowKind::Utf8 => DataType::Utf8,
    })
}

fn column_array(column: &ColumnOut, rows: &[Vec<Cell>], index: usize) -> Result<arrow_array::ArrayRef, Fail> {
    match column.kind {
        ArrowKind::Int64 => {
            let values = rows.iter().map(|row| match &row[index] {
                Cell::Null => Ok(None),
                Cell::I64(value) => Ok(Some(*value)),
                _ => Err(Fail::new("value_shape", column.name.clone())),
            }).collect::<Result<Vec<_>, Fail>>()?;
            Ok(Arc::new(Int64Array::from(values)))
        }
        ArrowKind::Decimal { precision, scale } => {
            let values = rows.iter().map(|row| match &row[index] {
                Cell::Null => Ok(None),
                Cell::Decimal(value) => Ok(Some(*value)),
                _ => Err(Fail::new("value_shape", column.name.clone())),
            }).collect::<Result<Vec<_>, Fail>>()?;
            let array = Decimal128Array::from(values)
                .with_precision_and_scale(precision, i8::try_from(scale).map_err(|_| Fail::new("decimal", "scale"))?)
                .map_err(|err| Fail::new("decimal_precision", err.to_string()))?;
            Ok(Arc::new(array))
        }
        ArrowKind::Date32 => {
            let values = rows.iter().map(|row| match &row[index] {
                Cell::Null => Ok(None),
                Cell::Date(value) => Ok(Some(*value)),
                _ => Err(Fail::new("value_shape", column.name.clone())),
            }).collect::<Result<Vec<_>, Fail>>()?;
            Ok(Arc::new(Date32Array::from(values)))
        }
        ArrowKind::NaiveTimestampSecond => {
            let values = rows.iter().map(|row| match &row[index] {
                Cell::Null => Ok(None),
                Cell::NaiveSecond(value) => Ok(Some(*value)),
                _ => Err(Fail::new("value_shape", column.name.clone())),
            }).collect::<Result<Vec<_>, Fail>>()?;
            Ok(Arc::new(TimestampSecondArray::from(values)))
        }
        ArrowKind::Utf8 => {
            let values = rows.iter().map(|row| match &row[index] {
                Cell::Null => Ok(None),
                Cell::Text(value) => Ok(Some(value.clone())),
                _ => Err(Fail::new("value_shape", column.name.clone())),
            }).collect::<Result<Vec<_>, Fail>>()?;
            Ok(Arc::new(StringArray::from(values)))
        }
    }
}

pub fn estimate_row_bytes(row: &[Cell]) -> u64 {
    row.iter().map(|cell| match cell {
        Cell::Null => 1,
        Cell::I64(_) | Cell::Date(_) | Cell::NaiveSecond(_) => 8,
        Cell::Decimal(value) => value.to_string().len() as u64,
        Cell::Text(value) => value.len() as u64,
    }).sum()
}
