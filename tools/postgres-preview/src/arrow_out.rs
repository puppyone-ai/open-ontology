//! Arrow IPC for logistics rows read from PostgreSQL.
//!
//! UUID values are 16-byte binary. Money-scale amounts are decimal128(38, 4).
//! Timestamps are microseconds since the Unix epoch with time zone UTC.
//! `admitted_extract` is stored in the schema metadata. A type sample sets it
//! false and is not an admitted import.

use std::io::Cursor;
use std::sync::Arc;

use arrow_array::{
    Array, BinaryArray, Decimal128Array, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray,
};
use arrow_ipc::reader::StreamReader;
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use arrow_ipc::MetadataVersion;
use arrow_schema::{DataType, Field, Schema, TimeUnit};

use crate::error::Fail;

#[cfg(test)]
use crate::logistics::format_scale4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArrowRow {
    pub shipment_id: i64,
    pub shipment_token: Vec<u8>,
    pub cost_amount: Option<i128>,
    pub weight_grams: Option<i64>,
    pub created_at_us: Option<i64>,
    pub seal: Option<Vec<u8>>,
    pub comment: Option<String>,
}

pub fn write_logistics_batch(rows: &[ArrowRow], admitted_extract: bool) -> Result<Vec<u8>, Fail> {
    if rows.is_empty() {
        return Err(Fail::new("arrow.empty", 8));
    }
    for row in rows {
        if row.shipment_token.len() != 16 {
            return Err(Fail::new("arrow.uuid", 5));
        }
    }
    let mut metadata = std::collections::HashMap::new();
    metadata.insert(
        "oo.postgres-preview.admitted_extract".to_owned(),
        if admitted_extract { "true" } else { "false" }.to_owned(),
    );
    let schema = Arc::new(Schema::new(vec![
        Field::new("shipment_id", DataType::Int64, false),
        Field::new("shipment_token", DataType::Binary, false),
        Field::new("cost_amount", DataType::Decimal128(38, 4), true),
        Field::new("weight_grams", DataType::Int64, true),
        Field::new(
            "created_at",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            true,
        ),
        Field::new("seal", DataType::Binary, true),
        Field::new("comment", DataType::Utf8, true),
    ]).with_metadata(metadata));
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int64Array::from(rows.iter().map(|row| row.shipment_id).collect::<Vec<_>>())),
            Arc::new(BinaryArray::from(
                rows.iter().map(|row| Some(row.shipment_token.as_slice())).collect::<Vec<_>>(),
            )),
            Arc::new(
                Decimal128Array::from(rows.iter().map(|row| row.cost_amount).collect::<Vec<_>>())
                    .with_precision_and_scale(38, 4)
                    .map_err(|_| Fail::new("arrow.decimal", 5))?,
            ),
            Arc::new(Int64Array::from(rows.iter().map(|row| row.weight_grams).collect::<Vec<_>>())),
            Arc::new(
                TimestampMicrosecondArray::from(rows.iter().map(|row| row.created_at_us).collect::<Vec<_>>())
                    .with_timezone("UTC"),
            ),
            Arc::new(BinaryArray::from(
                rows.iter()
                    .map(|row| row.seal.as_deref())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                rows.iter().map(|row| row.comment.as_deref()).collect::<Vec<_>>(),
            )),
        ],
    )
    .map_err(|_| Fail::new("arrow.ipc", 1))?;
    let options = IpcWriteOptions::try_new(8, false, MetadataVersion::V5).map_err(|_| Fail::new("arrow.ipc", 1))?;
    let mut buffer = Vec::new();
    let mut writer = StreamWriter::try_new_with_options(&mut buffer, schema.as_ref(), options)
        .map_err(|_| Fail::new("arrow.ipc", 1))?;
    writer.write(&batch).map_err(|_| Fail::new("arrow.ipc", 1))?;
    writer.finish().map_err(|_| Fail::new("arrow.ipc", 1))?;
    Ok(buffer)
}

pub fn read_logistics_batch(bytes: &[u8]) -> Result<(bool, Vec<ArrowRow>), Fail> {
    let reader = StreamReader::try_new(Cursor::new(bytes), None).map_err(|_| Fail::new("arrow.ipc", 1))?;
    let mut rows = Vec::new();
    let mut admitted = None;
    for batch in reader {
        let batch = batch.map_err(|_| Fail::new("arrow.ipc", 1))?;
        let schema = batch.schema();
        let flag = schema
            .metadata()
            .get("oo.postgres-preview.admitted_extract")
            .map(String::as_str);
        let flag = match flag {
            Some("true") => true,
            Some("false") => false,
            _ => return Err(Fail::new("arrow.ipc", 1)),
        };
        if admitted.is_some_and(|seen| seen != flag) {
            return Err(Fail::new("arrow.ipc", 1));
        }
        admitted = Some(flag);
        let ids = int64(&batch, 0)?;
        let tokens = binary(&batch, 1)?;
        let costs = decimal(&batch, 2)?;
        let weights = int64_opt(&batch, 3)?;
        let times = timestamp(&batch, 4)?;
        let seals = binary_opt(&batch, 5)?;
        let comments = utf8_opt(&batch, 6)?;
        if ids.len() != tokens.len() {
            return Err(Fail::new("arrow.ipc", 1));
        }
        for index in 0..ids.len() {
            let token = tokens[index].clone().ok_or(Fail::new("arrow.uuid", 5))?;
            if token.len() != 16 {
                return Err(Fail::new("arrow.uuid", 5));
            }
            rows.push(ArrowRow {
                shipment_id: ids[index].ok_or(Fail::new("arrow.ipc", 1))?,
                shipment_token: token,
                cost_amount: costs[index],
                weight_grams: weights[index],
                created_at_us: times[index],
                seal: seals[index].clone(),
                comment: comments[index].clone(),
            });
        }
    }
    let admitted = admitted.ok_or(Fail::new("arrow.empty", 8))?;
    if rows.is_empty() {
        return Err(Fail::new("arrow.empty", 8));
    }
    Ok((admitted, rows))
}

pub fn parse_uuid(text: &str) -> Result<Vec<u8>, Fail> {
    let hex: String = text.chars().filter(|char| *char != '-').collect();
    if hex.len() != 32 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Fail::new("arrow.uuid", 5));
    }
    let mut out = Vec::with_capacity(16);
    let bytes = hex.as_bytes();
    for index in 0..16 {
        let pair = std::str::from_utf8(&bytes[index * 2..index * 2 + 2]).map_err(|_| Fail::new("arrow.uuid", 5))?;
        out.push(u8::from_str_radix(pair, 16).map_err(|_| Fail::new("arrow.uuid", 5))?);
    }
    Ok(out)
}

pub fn parse_utc_micros(text: &str) -> Result<i64, Fail> {
    let bytes = text.as_bytes();
    if bytes.len() != 20 || bytes[10] != b'T' || bytes[19] != b'Z' {
        return Err(Fail::new("arrow.time", 5));
    }
    let year = number(&text[0..4])?;
    let month = number(&text[5..7])?;
    let day = number(&text[8..10])?;
    let hour = number(&text[11..13])?;
    let minute = number(&text[14..16])?;
    let second = number(&text[17..19])?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return Err(Fail::new("arrow.time", 5));
    }
    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second);
    seconds.checked_mul(1_000_000).ok_or(Fail::new("arrow.time", 5))
}

fn number(text: &str) -> Result<i32, Fail> {
    if (text.len() != 2 && text.len() != 4) || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Fail::new("arrow.time", 5));
    }
    text.parse().map_err(|_| Fail::new("arrow.time", 5))
}

fn days_from_civil(mut year: i32, month: i32, day: i32) -> i64 {
    year -= i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    i64::from(era) * 146_097 + i64::from(day_of_era) - 719_468
}

fn int64(batch: &RecordBatch, index: usize) -> Result<Vec<Option<i64>>, Fail> {
    let array = batch.column(index).as_any().downcast_ref::<Int64Array>().ok_or(Fail::new("arrow.ipc", 1))?;
    Ok((0..array.len()).map(|row| if array.is_null(row) { None } else { Some(array.value(row)) }).collect())
}

fn int64_opt(batch: &RecordBatch, index: usize) -> Result<Vec<Option<i64>>, Fail> {
    int64(batch, index)
}

fn timestamp(batch: &RecordBatch, index: usize) -> Result<Vec<Option<i64>>, Fail> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<TimestampMicrosecondArray>()
        .ok_or(Fail::new("arrow.ipc", 1))?;
    Ok((0..array.len()).map(|row| if array.is_null(row) { None } else { Some(array.value(row)) }).collect())
}

fn decimal(batch: &RecordBatch, index: usize) -> Result<Vec<Option<i128>>, Fail> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .ok_or(Fail::new("arrow.ipc", 1))?;
    if array.precision() != 38 || array.scale() != 4 {
        return Err(Fail::new("arrow.decimal", 5));
    }
    Ok((0..array.len()).map(|row| if array.is_null(row) { None } else { Some(array.value(row)) }).collect())
}

fn binary(batch: &RecordBatch, index: usize) -> Result<Vec<Option<Vec<u8>>>, Fail> {
    let array = batch.column(index).as_any().downcast_ref::<BinaryArray>().ok_or(Fail::new("arrow.ipc", 1))?;
    Ok((0..array.len())
        .map(|row| if array.is_null(row) { None } else { Some(array.value(row).to_vec()) })
        .collect())
}

fn binary_opt(batch: &RecordBatch, index: usize) -> Result<Vec<Option<Vec<u8>>>, Fail> {
    binary(batch, index)
}

fn utf8_opt(batch: &RecordBatch, index: usize) -> Result<Vec<Option<String>>, Fail> {
    let array = batch.column(index).as_any().downcast_ref::<StringArray>().ok_or(Fail::new("arrow.ipc", 1))?;
    Ok((0..array.len())
        .map(|row| if array.is_null(row) { None } else { Some(array.value(row).to_owned()) })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use arrow_ipc::reader::StreamReader;

    use super::*;
    use crate::logistics::{parse_scale4, EXACT_COST};

    #[test]
    fn decimal_uuid_null_and_timestamp_round_trip() {
        let exact = parse_scale4(EXACT_COST).unwrap();
        let rows = vec![
            ArrowRow {
                shipment_id: 1,
                shipment_token: parse_uuid("00000001-0000-4000-8000-000000000001").unwrap(),
                cost_amount: Some(exact),
                weight_grams: Some(i64::MIN),
                created_at_us: Some(-62_135_596_800_000_000),
                seal: Some(vec![]),
                comment: None,
            },
            ArrowRow {
                shipment_id: 17,
                shipment_token: parse_uuid("00000011-0000-4000-8000-000000000011").unwrap(),
                cost_amount: None,
                weight_grams: None,
                created_at_us: None,
                seal: None,
                comment: Some("waybill".to_owned()),
            },
        ];
        let bytes = write_logistics_batch(&rows, false).unwrap();
        let reader = StreamReader::try_new(Cursor::new(bytes.as_slice()), None).unwrap();
        assert_eq!(reader.schema().field(0).name(), "shipment_id");
        drop(reader);
        let (admitted, read) = read_logistics_batch(&bytes).unwrap();
        assert!(!admitted);
        assert_eq!(read, rows);
        assert_eq!(format_scale4(read[0].cost_amount.unwrap()), EXACT_COST);
        assert_eq!(parse_utc_micros("1970-01-01T00:00:00Z").unwrap(), 0);
        assert_eq!(parse_utc_micros("2000-01-01T00:00:00Z").unwrap(), 946_684_800_000_000);
        assert_eq!(parse_utc_micros("0001-01-01T00:00:00Z").unwrap(), -62_135_596_800_000_000);
        assert!(write_logistics_batch(&[], false).is_err());
    }
}
