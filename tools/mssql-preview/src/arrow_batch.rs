//! Arrow IPC for values already decoded by the precision rules.
//! A fixture is a codec input. It is not a native read and it is not GOLD.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::builder::FixedSizeBinaryBuilder;
use arrow_array::{
    ArrayRef, BinaryArray, BooleanArray, Decimal128Array, Int32Array, Int64Array, RecordBatch,
    StringArray, Time64NanosecondArray,
};
use arrow_ipc::reader::StreamReader;
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use arrow_ipc::MetadataVersion;
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::fail::Fail;
use crate::types::{
    collation_case_insensitive, datetime2_ticks, guid_wire_from_rfc, time_nanoseconds, unscaled_fits, Cell,
    SqlKind,
};

pub const FIXTURE_PROTOCOL: &str = "oo.mssql-preview.encode-fixture/v1";
pub const BATCH_PROTOCOL: &str = "oo.mssql-preview.arrow-batch/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnSpec {
    pub name: String,
    pub kind: SqlKind,
    pub nullable: bool,
    pub precision: u8,
    pub scale: u8,
    pub collation: Option<String>,
    pub is_identity: bool,
    pub sql_type_name: String,
    pub max_length: Option<i32>,
}

#[derive(Debug)]
pub struct ArrowPackage {
    pub ipc: Vec<u8>,
    pub manifest: Value,
}

pub fn encode_cells(columns: &[ColumnSpec], rows: &[Vec<Cell>]) -> Result<ArrowPackage, Fail> {
    for (index, row) in rows.iter().enumerate() {
        if row.len() != columns.len() {
            return Err(Fail::new("row_width", format!("row {index} width")));
        }
    }
    let fields = columns
        .iter()
        .map(field_for)
        .collect::<Result<Vec<_>, Fail>>()?;
    let mut schema_meta = HashMap::new();
    schema_meta.insert("oo.mssql-preview.protocol".to_string(), BATCH_PROTOCOL.to_string());
    schema_meta.insert("native_source".to_string(), "false".to_string());
    schema_meta.insert("gold".to_string(), "false".to_string());
    schema_meta.insert("pg_timestamp6".to_string(), "refused".to_string());
    let schema = Arc::new(Schema::new(fields).with_metadata(schema_meta));
    let arrays = columns
        .iter()
        .enumerate()
        .map(|(index, column)| column_array(column, rows, index))
        .collect::<Result<Vec<_>, Fail>>()?;
    let batch = RecordBatch::try_new(Arc::clone(&schema), arrays)
        .map_err(|_| Fail::new("arrow_batch", "arrow batch could not be built"))?;
    let mut ipc = Vec::new();
    let options = IpcWriteOptions::try_new(8, false, MetadataVersion::V5)
        .map_err(|_| Fail::new("arrow_ipc", "arrow writer options were refused"))?;
    let mut writer = StreamWriter::try_new_with_options(&mut ipc, schema.as_ref(), options)
        .map_err(|_| Fail::new("arrow_ipc", "arrow writer could not start"))?;
    writer
        .write(&batch)
        .map_err(|_| Fail::new("arrow_ipc", "arrow batch could not be written"))?;
    writer
        .finish()
        .map_err(|_| Fail::new("arrow_ipc", "arrow stream could not be finished"))?;
    let hash = Sha256::digest(&ipc);
    let ipc_sha256 = hash.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let manifest = json!({
        "protocol": BATCH_PROTOCOL,
        "native_source": false,
        "gold": false,
        "executed_against_sqlserver": false,
        "pg_timestamp6": "refused",
        "rows": rows.len(),
        "ipc_sha256": ipc_sha256,
        "fields": columns.iter().map(field_manifest).collect::<Vec<_>>()
    });
    Ok(ArrowPackage { ipc, manifest })
}

pub fn encode_fixture(bytes: &[u8]) -> Result<ArrowPackage, Fail> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| Fail::new("invalid_fixture", "fixture json could not be read"))?;
    if value.get("protocol").and_then(Value::as_str) != Some(FIXTURE_PROTOCOL) {
        return Err(Fail::new("invalid_fixture", "fixture protocol is not recognized"));
    }
    if value.get("native_source").and_then(Value::as_bool) != Some(false) {
        return Err(Fail::new(
            "fixture_not_native",
            "an encode fixture cannot claim native_source",
        ));
    }
    if value.get("gold").and_then(Value::as_bool) == Some(true) {
        return Err(Fail::new("fixture_is_not_gold", "an encode fixture is not GOLD"));
    }
    let columns = value
        .get("columns")
        .and_then(Value::as_array)
        .ok_or_else(|| Fail::new("invalid_fixture", "columns are required"))?;
    let specs = columns.iter().map(spec_from_json).collect::<Result<Vec<_>, Fail>>()?;
    let rows_json = value
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| Fail::new("invalid_fixture", "rows are required"))?;
    let mut rows = Vec::new();
    for row in rows_json {
        let cells = row
            .as_array()
            .ok_or_else(|| Fail::new("invalid_fixture", "a row must be an array"))?;
        if cells.len() != specs.len() {
            return Err(Fail::new("row_width", "fixture row width"));
        }
        let mut decoded = Vec::new();
        for (spec, cell) in specs.iter().zip(cells) {
            decoded.push(cell_from_fixture(spec, cell)?);
        }
        rows.push(decoded);
    }
    let mut package = encode_cells(&specs, &rows)?;
    if let Some(object) = package.manifest.as_object_mut() {
        object.insert("executed_against_sqlserver".to_string(), Value::Bool(false));
    }
    Ok(package)
}

fn spec_from_json(value: &Value) -> Result<ColumnSpec, Fail> {
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| Fail::new("invalid_fixture", "column name is required"))?;
    let sql_type = value
        .get("sql_type")
        .and_then(Value::as_str)
        .ok_or_else(|| Fail::new("invalid_fixture", "sql_type is required"))?;
    let kind = match sql_type {
        "nvarchar" | "nchar" | "ntext" | "sysname" | "xml" => SqlKind::Nvarchar,
        "binary" | "varbinary" => SqlKind::Binary,
        "decimal" | "numeric" | "money" | "smallmoney" => SqlKind::Decimal,
        "time" => SqlKind::Time,
        "datetime2" => SqlKind::Datetime2,
        "date" => SqlKind::Date,
        "uniqueidentifier" => SqlKind::Uniqueidentifier,
        "bit" => SqlKind::Bit,
        "tinyint" | "smallint" | "int" | "bigint" => SqlKind::Int,
        _ => return Err(Fail::new("unsupported_type", "fixture sql_type is refused")),
    };
    let precision = u8_field(value, "precision")?;
    let scale = u8_field(value, "scale")?;
    if matches!(kind, SqlKind::Decimal) && (precision == 0 || precision > 38) {
        return Err(Fail::new("decimal_precision", "decimal precision must be 1..=38"));
    }
    Ok(ColumnSpec {
        name: name.to_string(),
        kind,
        nullable: value.get("nullable").and_then(Value::as_bool).unwrap_or(true),
        precision,
        scale,
        collation: value.get("collation").and_then(Value::as_str).map(str::to_string),
        is_identity: value.get("is_identity").and_then(Value::as_bool).unwrap_or(false),
        sql_type_name: sql_type.to_string(),
        max_length: value.get("max_length").and_then(Value::as_i64).and_then(|n|i32::try_from(n).ok()),
    })
}

fn u8_field(value: &Value, name: &str) -> Result<u8, Fail> {
    let Some(number) = value.get(name).and_then(Value::as_u64) else {
        return Ok(0);
    };
    u8::try_from(number).map_err(|_| Fail::new("invalid_fixture", "numeric field does not fit"))
}

fn cell_from_fixture(spec: &ColumnSpec, value: &Value) -> Result<Cell, Fail> {
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| Fail::new("invalid_fixture", "cell kind is required"))?;
    if kind == "null" {
        if !spec.nullable {
            return Err(Fail::new("null_not_nullable", "null is not allowed in this column"));
        }
        return Ok(Cell::Null);
    }
    match (&spec.kind, kind) {
        (SqlKind::Nvarchar, "nvarchar") => Ok(Cell::Text(required_string(value, "value")?)),
        (SqlKind::Binary, "binary") => Ok(Cell::Bytes(decode_hex(&required_string(value, "hex")?)?)),
        (SqlKind::Decimal, "decimal") => {
            let unscaled = required_string(value, "unscaled")?
                .parse::<i128>()
                .map_err(|_| Fail::new("decimal_precision", "unscaled decimal is not an integer"))?;
            if !unscaled_fits(unscaled, spec.precision) {
                return Err(Fail::new("decimal_precision", "unscaled decimal exceeds precision"));
            }
            Ok(Cell::Decimal(unscaled))
        }
        (SqlKind::Time, "time") => {
            let increments = required_u64(value, "increments")?;
            let scale = required_u64(value, "scale")?;
            if scale != u64::from(spec.scale) {
                return Err(Fail::new("time_scale", "time scale does not match the column"));
            }
            Ok(Cell::TimeNanos(time_nanoseconds(increments, spec.scale)?))
        }
        (SqlKind::Datetime2, "datetime2") => {
            let days = required_u64(value, "days")?;
            let increments = required_u64(value, "increments")?;
            let scale = required_u64(value, "scale")?;
            if scale != u64::from(spec.scale) || spec.scale > 7 {
                return Err(Fail::new("datetime2_scale", "datetime2 scale does not match the column"));
            }
            let days = u32::try_from(days).map_err(|_| Fail::new("date_range", "days do not fit"))?;
            Ok(Cell::Datetime2Ticks(datetime2_ticks(days, increments, spec.scale)?))
        }
        (SqlKind::Date, "date") => {
            let days = u32::try_from(required_u64(value, "days")?)
                .map_err(|_| Fail::new("date_range", "days do not fit"))?;
            Ok(Cell::DateDays(i32::try_from(days).map_err(|_| Fail::new("date_range", "days do not fit"))?))
        }
        (SqlKind::Uniqueidentifier, "uniqueidentifier") => {
            let hex = required_string(value, "rfc")?;
            let bytes = decode_hex(&hex)?;
            if bytes.len() != 16 {
                return Err(Fail::new("guid_type", "rfc guid must be 16 bytes"));
            }
            let mut rfc = [0u8; 16];
            rfc.copy_from_slice(&bytes);
            Ok(Cell::GuidWire(guid_wire_from_rfc(rfc)))
        }
        (SqlKind::Bit, "bit") => Ok(Cell::Bit(
            value
                .get("value")
                .and_then(Value::as_bool)
                .ok_or_else(|| Fail::new("bit_type", "bit value is required"))?,
        )),
        (SqlKind::Int, "int") => Ok(Cell::Int(
            value
                .get("value")
                .and_then(Value::as_i64)
                .ok_or_else(|| Fail::new("integer_type", "int value is required"))?,
        )),
        _ => Err(Fail::new("invalid_fixture", "cell kind does not match the column")),
    }
}

fn required_string(value: &Value, name: &str) -> Result<String, Fail> {
    value
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Fail::new("invalid_fixture", "fixture string field is missing"))
}

fn required_u64(value: &Value, name: &str) -> Result<u64, Fail> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| Fail::new("invalid_fixture", "fixture number field is missing"))
}

fn decode_hex(value: &str) -> Result<Vec<u8>, Fail> {
    if value.len() % 2 != 0 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Fail::new("binary_type", "hex payload is not even hex"));
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let pair = std::str::from_utf8(&bytes[index..index + 2])
            .map_err(|_| Fail::new("binary_type", "hex payload is not even hex"))?;
        out.push(u8::from_str_radix(pair, 16).map_err(|_| Fail::new("binary_type", "hex payload is not even hex"))?);
        index += 2;
    }
    Ok(out)
}

fn field_for(column: &ColumnSpec) -> Result<Field, Fail> {
    let data_type = match column.kind {
        SqlKind::Nvarchar => DataType::Utf8,
        SqlKind::Binary => DataType::Binary,
        SqlKind::Decimal => {
            let scale = i8::try_from(column.scale).map_err(|_| Fail::new("decimal_scale", "scale does not fit"))?;
            DataType::Decimal128(column.precision, scale)
        }
        SqlKind::Time => DataType::Time64(TimeUnit::Nanosecond),
        SqlKind::Datetime2 | SqlKind::Int => DataType::Int64,
        SqlKind::Date => DataType::Int32,
        SqlKind::Uniqueidentifier => DataType::FixedSizeBinary(16),
        SqlKind::Bit => DataType::Boolean,
    };
    let mut metadata = HashMap::new();
    metadata.insert("sqlserver_type".to_string(), column.sql_type_name.clone());
    if let Some(length)=column.max_length {metadata.insert("source_max_length_bytes".into(),length.to_string());}
    metadata.insert("is_identity".to_string(), column.is_identity.to_string());
    match column.kind {
        SqlKind::Datetime2 => {
            metadata.insert("unit".to_string(), "100ns".to_string());
            metadata.insert("epoch".to_string(), "0001-01-01".to_string());
            metadata.insert("pg_timestamp6".to_string(), "refused".to_string());
            metadata.insert("scale".to_string(), column.scale.to_string());
        }
        SqlKind::Date => {
            metadata.insert("unit".to_string(), "days".to_string());
            metadata.insert("epoch".to_string(), "0001-01-01".to_string());
        }
        SqlKind::Time => {
            metadata.insert("unit".to_string(), "ns".to_string());
            metadata.insert("scale".to_string(), column.scale.to_string());
            metadata.insert(
                "conversion".to_string(),
                "time increments * 10^(9-scale); exact for scale <= 7".to_string(),
            );
        }
        SqlKind::Nvarchar => {
            if let Some(collation) = &column.collation {
                metadata.insert("source_collation".to_string(), collation.clone());
                metadata.insert(
                    "case_insensitive".to_string(),
                    collation_case_insensitive(collation).to_string(),
                );
            }
            metadata.insert("sort_applied".to_string(), "false".to_string());
            metadata.insert("bytes_preserved".to_string(), "true".to_string());
        }
        SqlKind::Binary => {
            metadata.insert("encoding".to_string(), "raw".to_string());
        }
        SqlKind::Uniqueidentifier => {
            metadata.insert("byte_order".to_string(), "sqlserver_mixed_endian".to_string());
            metadata.insert("arrow_uuid_type".to_string(), "not_used".to_string());
        }
        SqlKind::Decimal => {
            metadata.insert("exact".to_string(), "true".to_string());
        }
        SqlKind::Bit | SqlKind::Int => {}
    }
    Ok(Field::new(column.name.clone(), data_type, column.nullable).with_metadata(metadata))
}

fn field_manifest(column: &ColumnSpec) -> Value {
    json!({
        "name": column.name,
        "sql_type": column.sql_type_name,
        "precision": column.precision,
        "scale": column.scale,
        "is_identity": column.is_identity,
        "source_max_length_bytes": column.max_length
    })
}

fn column_array(column: &ColumnSpec, rows: &[Vec<Cell>], index: usize) -> Result<ArrayRef, Fail> {
    match column.kind {
        SqlKind::Nvarchar => {
            let values = rows.iter().map(|row| text_cell(&row[index])).collect::<Result<Vec<_>, _>>()?;
            Ok(Arc::new(StringArray::from(values)))
        }
        SqlKind::Binary => {
            let owned = rows
                .iter()
                .map(|row| match &row[index] {
                    Cell::Null => Ok(None),
                    Cell::Bytes(value) => Ok(Some(value.clone())),
                    _ => Err(Fail::new("binary_type", "cell is not raw bytes")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let values: Vec<Option<&[u8]>> = owned.iter().map(|value| value.as_deref()).collect();
            Ok(Arc::new(BinaryArray::from(values)))
        }
        SqlKind::Decimal => {
            let values = rows
                .iter()
                .map(|row| match &row[index] {
                    Cell::Null => Ok(None),
                    Cell::Decimal(value) => Ok(Some(*value)),
                    _ => Err(Fail::new("decimal_type", "cell is not decimal")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let scale = i8::try_from(column.scale).map_err(|_| Fail::new("decimal_scale", "scale does not fit"))?;
            let array = Decimal128Array::from(values)
                .with_precision_and_scale(column.precision, scale)
                .map_err(|_| Fail::new("decimal_precision", "decimal128 refused the precision"))?;
            Ok(Arc::new(array))
        }
        SqlKind::Time => {
            let values = rows
                .iter()
                .map(|row| match &row[index] {
                    Cell::Null => Ok(None),
                    Cell::TimeNanos(value) => Ok(Some(*value)),
                    _ => Err(Fail::new("time_type", "cell is not time")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Arc::new(Time64NanosecondArray::from(values)))
        }
        SqlKind::Datetime2 => {
            let values = rows
                .iter()
                .map(|row| match &row[index] {
                    Cell::Null => Ok(None),
                    Cell::Datetime2Ticks(value) => Ok(Some(*value)),
                    _ => Err(Fail::new("datetime2_type", "cell is not datetime2")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Arc::new(Int64Array::from(values)))
        }
        SqlKind::Int => {
            let values = rows
                .iter()
                .map(|row| match &row[index] {
                    Cell::Null => Ok(None),
                    Cell::Int(value) => Ok(Some(*value)),
                    _ => Err(Fail::new("integer_type", "cell is not an integer")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Arc::new(Int64Array::from(values)))
        }
        SqlKind::Date => {
            let values = rows
                .iter()
                .map(|row| match &row[index] {
                    Cell::Null => Ok(None),
                    Cell::DateDays(value) => Ok(Some(*value)),
                    _ => Err(Fail::new("date_type", "cell is not a date")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Arc::new(Int32Array::from(values)))
        }
        SqlKind::Uniqueidentifier => {
            let mut builder = FixedSizeBinaryBuilder::with_capacity(rows.len(), 16);
            for row in rows {
                match &row[index] {
                    Cell::Null => builder.append_null(),
                    Cell::GuidWire(value) => builder
                        .append_value(value)
                        .map_err(|_| Fail::new("guid_type", "guid could not be appended"))?,
                    _ => return Err(Fail::new("guid_type", "cell is not a guid")),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        SqlKind::Bit => {
            let values = rows
                .iter()
                .map(|row| match &row[index] {
                    Cell::Null => Ok(None),
                    Cell::Bit(value) => Ok(Some(*value)),
                    _ => Err(Fail::new("bit_type", "cell is not a bit")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Arc::new(BooleanArray::from(values)))
        }
    }
}

fn text_cell(cell: &Cell) -> Result<Option<String>, Fail> {
    match cell {
        Cell::Null => Ok(None),
        Cell::Text(value) => Ok(Some(value.clone())),
        _ => Err(Fail::new("nvarchar_type", "cell is not nvarchar")),
    }
}

pub fn read_ipc(bytes: &[u8]) -> Result<RecordBatch, Fail> {
    let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None)
        .map_err(|_| Fail::new("arrow_ipc", "arrow stream could not be read"))?;
    let mut batches = reader.collect::<Result<Vec<_>, _>>().map_err(|_| {
        Fail::new("arrow_ipc", "arrow stream could not be read")
    })?;
    if batches.len() != 1 {
        return Err(Fail::new("arrow_ipc", "arrow stream did not contain one batch"));
    }
    Ok(batches.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{datetime2_ticks, guid_wire_from_rfc};
    use arrow_array::{Array, Decimal128Array, FixedSizeBinaryArray, Int64Array, StringArray};

    fn spec(name: &str, sql_type: &str, kind: SqlKind, precision: u8, scale: u8, collation: Option<&str>) -> ColumnSpec {
        ColumnSpec {
            name: name.to_string(),
            kind,
            nullable: true,
            precision,
            scale,
            collation: collation.map(str::to_string),
            is_identity: false,
            sql_type_name: sql_type.to_string(),
        max_length: None,
        }
    }

    #[test]
    fn arrow_roundtrip_keeps_100ns_text_bytes_decimal_and_wire_guid() {
        let ticks = datetime2_ticks(738_959, 452_961_234_567, 7).unwrap();
        let unscaled = 9_007_199_254_740_993_i128 * 10_000_000_000 + 1;
        let columns = vec![
            spec("name_ci", "nvarchar", SqlKind::Nvarchar, 0, 0, Some("Latin1_General_CI_AS")),
            spec("key_bin", "binary", SqlKind::Binary, 0, 0, None),
            spec("amt", "decimal", SqlKind::Decimal, 26, 10, None),
            spec("dt7", "datetime2", SqlKind::Datetime2, 0, 7, None),
            spec("id", "uniqueidentifier", SqlKind::Uniqueidentifier, 0, 0, None),
        ];
        let wire = guid_wire_from_rfc([
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        ]);
        let rows = vec![vec![
            Cell::Text("A".to_string()),
            Cell::Bytes(vec![0x00, 0x00, 0x00, 0xff]),
            Cell::Decimal(unscaled),
            Cell::Datetime2Ticks(ticks),
            Cell::GuidWire(wire),
        ], vec![
            Cell::Text("a".to_string()),
            Cell::Bytes(vec![0xff, 0x00, 0x00, 0x00]),
            Cell::Null,
            Cell::Datetime2Ticks(ticks - 7),
            Cell::Null,
        ], vec![
            Cell::Text("A ".to_string()),
            Cell::Null,
            Cell::Decimal(unscaled),
            Cell::Null,
            Cell::GuidWire(wire),
        ]];
        let package = encode_cells(&columns, &rows).unwrap();
        assert_eq!(package.manifest["native_source"], false);
        assert_eq!(package.manifest["gold"], false);
        assert_eq!(package.manifest["pg_timestamp6"], "refused");
        assert!(!package.manifest.to_string().contains("\"admitted\""));
        let batch = read_ipc(&package.ipc).unwrap();
        let schema = batch.schema();
        assert_eq!(schema.metadata().get("native_source").map(String::as_str), Some("false"));
        let dt = schema.field_with_name("dt7").unwrap();
        assert_eq!(dt.metadata().get("unit").map(String::as_str), Some("100ns"));
        assert_eq!(dt.metadata().get("pg_timestamp6").map(String::as_str), Some("refused"));
        assert_eq!(dt.data_type(), &DataType::Int64);
        let names = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(names.value(0), "A");
        assert_eq!(names.value(1), "a");
        assert_eq!(names.value(2), "A ");
        assert_ne!(names.value(0), names.value(1));
        let name_field = schema.field_with_name("name_ci").unwrap();
        assert_eq!(name_field.metadata().get("case_insensitive").map(String::as_str), Some("true"));
        assert_eq!(name_field.metadata().get("sort_applied").map(String::as_str), Some("false"));
        let amounts = batch.column(2).as_any().downcast_ref::<Decimal128Array>().unwrap();
        assert_eq!(amounts.value(0), unscaled);
        let ticks_array = batch.column(3).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(ticks_array.value(0) - ticks_array.value(1), 7);
        let guids = batch.column(4).as_any().downcast_ref::<FixedSizeBinaryArray>().unwrap();
        assert_eq!(guids.value(0), wire);
        assert_ne!(DataType::FixedSizeBinary(16), DataType::Utf8);
    }

    #[test]
    fn fixture_cannot_claim_native_or_gold() {
        let bad = br#"{"protocol":"oo.mssql-preview.encode-fixture/v1","native_source":true,"columns":[],"rows":[]}"#;
        assert_eq!(encode_fixture(bad).unwrap_err().code, "fixture_not_native");
    }
}
