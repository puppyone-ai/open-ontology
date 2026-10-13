use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use arrow_array::builder::{Date32Builder, Decimal128Builder, Int64Builder, StringBuilder};
use arrow_array::{RecordBatch, ArrayRef};
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use arrow_ipc::MetadataVersion;
use arrow_schema::{DataType, Field, Schema};

use crate::cell::{Cell, Kind};
use crate::fail::Fail;

#[derive(Clone, Copy, Debug)]
pub struct Column {
    pub name: &'static str,
    pub kind: Kind,
    pub nullable: bool,
}

pub fn write_file(columns: &[Column], rows: &[Vec<Cell>], metadata: HashMap<String, String>) -> Result<Vec<u8>, Fail> {
    for (index, row) in rows.iter().enumerate() {
        if row.len() != columns.len() {
            return Err(Fail::new("row_width", format!("row {index}")));
        }
    }
    let fields = columns
        .iter()
        .map(|column| {
            Ok(Field::new(column.name, arrow_type(column.kind)?, column.nullable))
        })
        .collect::<Result<Vec<_>, Fail>>()?;
    let schema = Arc::new(Schema::new(fields).with_metadata(metadata));
    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(columns.len());
    for (index, column) in columns.iter().enumerate() {
        let cells = rows.iter().map(|row| &row[index]);
        arrays.push(column_array(*column, cells)?);
    }
    let batch = RecordBatch::try_new(Arc::clone(&schema), arrays).map_err(|err| Fail::new("ipc", err.to_string()))?;
    let mut cursor = Cursor::new(Vec::new());
    {
        // The fixed importer reads a continuation stream and only skips 8-byte padding.
        let options = IpcWriteOptions::try_new(8, false, MetadataVersion::V5)
            .map_err(|err| Fail::new("ipc", err.to_string()))?;
        let mut writer = StreamWriter::try_new_with_options(&mut cursor, schema.as_ref(), options)
            .map_err(|err| Fail::new("ipc", err.to_string()))?;
        writer.write(&batch).map_err(|err| Fail::new("ipc", err.to_string()))?;
        writer.finish().map_err(|err| Fail::new("ipc", err.to_string()))?;
    }
    let bytes = cursor.into_inner();
    if !stream_framed(&bytes) {
        return Err(Fail::new("ipc", "missing stream eos"));
    }
    Ok(bytes)
}

fn stream_framed(bytes: &[u8]) -> bool {
    if bytes.len() < 16 || bytes.starts_with(b"ARROW1") {
        return false;
    }
    let continuation = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let meta_len = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let tail = &bytes[bytes.len() - 8..];
    continuation == -1 && meta_len > 0 && tail == [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]
}

fn arrow_type(kind: Kind) -> Result<DataType, Fail> {
    Ok(match kind {
        Kind::Int64 => DataType::Int64,
        Kind::Utf8 => DataType::Utf8,
        Kind::Date => DataType::Date32,
        Kind::Decimal { precision, scale } => {
            let scale = i8::try_from(scale).map_err(|_| Fail::new("unsupported_precision", "scale"))?;
            DataType::Decimal128(precision, scale)
        }
    })
}

fn column_array<'a>(column: Column, cells: impl Iterator<Item = &'a Cell>) -> Result<ArrayRef, Fail> {
    let cells: Vec<&Cell> = cells.collect();
    match column.kind {
        Kind::Int64 => {
            let mut builder = Int64Builder::with_capacity(cells.len());
            for cell in cells {
                match cell {
                    Cell::Null if column.nullable => builder.append_null(),
                    Cell::Int(value) => builder.append_value(*value),
                    _ => return Err(Fail::new("cell", column.name)),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        Kind::Utf8 => {
            let mut builder = StringBuilder::new();
            for cell in cells {
                match cell {
                    Cell::Null if column.nullable => builder.append_null(),
                    Cell::Text(value) => builder.append_value(value),
                    _ => return Err(Fail::new("cell", column.name)),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        Kind::Date => {
            let mut builder = Date32Builder::with_capacity(cells.len());
            for cell in cells {
                match cell {
                    Cell::Null if column.nullable => builder.append_null(),
                    Cell::Date(value) => builder.append_value(*value),
                    _ => return Err(Fail::new("cell", column.name)),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        Kind::Decimal { precision, scale } => {
            let scale_i = i8::try_from(scale).map_err(|_| Fail::new("unsupported_precision", "scale"))?;
            let mut builder = Decimal128Builder::with_capacity(cells.len())
                .with_precision_and_scale(precision, scale_i)
                .map_err(|err| Fail::new("ipc", err.to_string()))?;
            for cell in cells {
                match cell {
                    Cell::Null if column.nullable => builder.append_null(),
                    Cell::Decimal(value) => builder.append_value(*value),
                    _ => return Err(Fail::new("cell", column.name)),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::Cursor;

    use arrow_array::{Array, Date32Array, Decimal128Array, Int64Array, StringArray};
    use arrow_ipc::reader::StreamReader;

    use super::{stream_framed, write_file, Column};
    use crate::cell::{Cell, Kind};

    #[test]
    fn stream_roundtrip_keeps_exact_values() {
        let columns = [
            Column { name: "n", kind: Kind::Int64, nullable: false },
            Column { name: "d", kind: Kind::Decimal { precision: 18, scale: 4 }, nullable: true },
            Column { name: "day", kind: Kind::Date, nullable: false },
            Column { name: "s", kind: Kind::Utf8, nullable: false },
        ];
        let rows = vec![
            vec![Cell::Int(11), Cell::Decimal(425), Cell::Date(20523), Cell::Text(String::new())],
            vec![Cell::Int(13), Cell::Null, Cell::Date(20547), Cell::Text("日".to_string())],
        ];
        let mut meta = HashMap::new();
        meta.insert("oo.doris.final_keyword_required".to_string(), "false".to_string());
        let bytes = write_file(&columns, &rows, meta).unwrap();
        assert!(stream_framed(&bytes));
        let reader = StreamReader::try_new(Cursor::new(bytes), None).unwrap();
        let batch = reader.into_iter().next().unwrap().unwrap();
        assert_eq!(batch.schema().metadata().get("oo.doris.final_keyword_required").map(String::as_str), Some("false"));
        let numbers = batch.column(0).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(numbers.value(0), 11);
        let decimals = batch.column(1).as_any().downcast_ref::<Decimal128Array>().unwrap();
        assert_eq!(decimals.value(0), 425);
        assert!(decimals.is_null(1));
        let days = batch.column(2).as_any().downcast_ref::<Date32Array>().unwrap();
        assert_eq!(days.value(0), 20523);
        let text = batch.column(3).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(text.value(0), "");
        assert_eq!(text.value(1), "日");
    }
}
