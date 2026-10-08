use std::io::Cursor;
use std::sync::Arc;

use arrow::array::{ArrayRef, Decimal128Builder, Int32Array, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use arrow::ipc::writer::FileWriter;

use crate::analyze::Analysis;
use crate::fault::Fault;

pub struct Columns {
    pub parquet: Vec<u8>,
    pub arrow: Vec<u8>,
}

pub fn write_columns(analysis: &Analysis) -> Result<Columns, Fault> {
    let scale = analysis
        .groups
        .iter()
        .map(|group| group.value.scale)
        .max()
        .unwrap_or(0);
    let scale_i8 = i8::try_from(scale).map_err(|_| Fault::engine())?;
    if !(0..=38).contains(&scale_i8) {
        return Err(Fault::engine());
    }
    let mut keys = Vec::with_capacity(analysis.rows.len());
    let mut ordinals = Vec::with_capacity(analysis.rows.len());
    let mut coefficients = Vec::with_capacity(analysis.rows.len());
    for row in &analysis.rows {
        keys.push(row.key.clone());
        ordinals.push(row.ordinal);
        coefficients.push(aligned_i128(&row.value.coefficient, row.value.negative, row.value.scale, scale)?);
    }
    let schema = schema(scale_i8);
    let mut decimal = Decimal128Builder::new()
        .with_precision_and_scale(38_u8, scale_i8)
        .map_err(|_| Fault::engine())?;
    for value in &coefficients {
        decimal.append_value(*value);
    }
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from(keys)) as ArrayRef,
            Arc::new(Int32Array::from(ordinals)) as ArrayRef,
            Arc::new(decimal.finish()) as ArrayRef,
        ],
    )
    .map_err(|_| Fault::parquet())?;
    Ok(Columns {
        parquet: write_parquet(&batch, schema.clone())?,
        arrow: write_arrow(&batch, &schema)?,
    })
}

fn schema(scale: i8) -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("group_key", DataType::Utf8, false),
        Field::new("ordinal", DataType::Int32, false),
        Field::new("coefficient", DataType::Decimal128(38_u8, scale), false),
    ]))
}

fn aligned_i128(coefficient: &str, negative: bool, from: i32, to: i32) -> Result<i128, Fault> {
    if to < from || from < 0 {
        return Err(Fault::engine());
    }
    let extra = usize::try_from(to - from).map_err(|_| Fault::engine())?;
    if coefficient.len().saturating_add(extra) > 38 {
        return Err(Fault::engine());
    }
    let mut digits = coefficient.to_string();
    digits.extend(std::iter::repeat_n('0', extra));
    let mut value: i128 = 0;
    for byte in digits.bytes() {
        if !byte.is_ascii_digit() {
            return Err(Fault::engine());
        }
        value = value.checked_mul(10).ok_or(Fault::engine())?;
        value = value.checked_add(i128::from(byte - b'0')).ok_or(Fault::engine())?;
    }
    if negative && value != 0 {
        value = value.checked_neg().ok_or(Fault::engine())?;
    }
    Ok(value)
}

fn write_parquet(batch: &RecordBatch, schema: SchemaRef) -> Result<Vec<u8>, Fault> {
    use parquet::arrow::ArrowWriter;
    use parquet::basic::Compression;
    use parquet::file::properties::WriterProperties;

    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .set_created_by("local-analysis".to_string())
        .build();
    let mut cursor = Cursor::new(Vec::new());
    let mut writer = ArrowWriter::try_new(&mut cursor, schema, Some(props)).map_err(|_| Fault::parquet())?;
    writer.write(batch).map_err(|_| Fault::parquet())?;
    writer.close().map_err(|_| Fault::parquet())?;
    Ok(cursor.into_inner())
}

fn write_arrow(batch: &RecordBatch, schema: &Schema) -> Result<Vec<u8>, Fault> {
    let mut cursor = Cursor::new(Vec::new());
    let mut writer = FileWriter::try_new(&mut cursor, schema).map_err(|_| Fault::parquet())?;
    writer.write(batch).map_err(|_| Fault::parquet())?;
    writer.finish().map_err(|_| Fault::parquet())?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::write_columns;
    use crate::analyze::{Analysis, Contribution, Group};
    use crate::decimal::Decimal;
    use arrow::array::{Array, Decimal128Array, StringArray};
    use arrow::ipc::reader::FileReader;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::io::Cursor;

    fn dec(text: &str, negative: bool, coefficient: &str, scale: i32) -> Decimal {
        Decimal {
            negative,
            coefficient: coefficient.to_string(),
            scale,
            text: text.to_string(),
        }
    }

    #[test]
    fn parquet_and_arrow_store_the_same_signed_decimal() {
        let analysis = Analysis {
            groups: vec![Group {
                key: "甲".to_string(),
                text_key: true,
                value: dec("1.50", false, "150", 2),
            }],
            rows: vec![
                Contribution {
                    key: "甲".to_string(),
                    ordinal: 0,
                    value: dec("1.5", false, "15", 1),
                },
                Contribution {
                    key: "甲".to_string(),
                    ordinal: 0,
                    value: dec("-0.00", false, "0", 2),
                },
            ],
            null_groups: 0,
            null_values: 0,
            csv: Vec::new(),
        };
        let columns = write_columns(&analysis).unwrap();
        assert!(columns.parquet.starts_with(b"PAR1"));
        assert!(columns.arrow.windows(6).any(|window| window == b"ARROW1"));
        let dir = std::env::temp_dir().join(format!(
            "oo-a386-col-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("in.parquet");
        std::fs::write(&path, &columns.parquet).unwrap();
        let parquet = ParquetRecordBatchReaderBuilder::try_new(std::fs::File::open(&path).unwrap())
            .unwrap()
            .build()
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let arrow = FileReader::try_new(Cursor::new(columns.arrow), None)
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(parquet.num_rows(), 2);
        assert_eq!(arrow.num_rows(), 2);
        let keys = parquet.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        let values = parquet
            .column(2)
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let arrow_values = arrow.column(2).as_any().downcast_ref::<Decimal128Array>().unwrap();
        assert_eq!(keys.value(0), "甲");
        assert_eq!(values.value(0), 150);
        assert_eq!(values.value(1), 0);
        assert_eq!(arrow_values.value(0), values.value(0));
        assert_eq!(arrow_values.value(1), values.value(1));
    }
}
