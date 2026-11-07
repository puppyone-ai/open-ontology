//! Bounded, native-typed Arrow handoff. Unsupported target mappings fail closed.
use super::chunk::{Result, decimal, digest, text};
use arrow_array::{
    ArrayRef, BinaryArray, Decimal128Array, Int64Array, RecordBatch, StringArray,
    TimestampMicrosecondArray,
};
use arrow_ipc::{reader::StreamReader, writer::StreamWriter};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    path::Path,
    sync::Arc,
};

pub(super) fn coefficient(value: &str, precision: u8, scale: i8) -> Result<i128> {
    let value = decimal(value)?;
    let negative = value.starts_with('-');
    let bare = value.trim_start_matches('-');
    let (whole, frac) = bare.split_once('.').unwrap_or((bare, ""));
    if scale < 0 {
        let zeros = (-i16::from(scale)) as usize;
        if !frac.is_empty()
            || (whole != "0" && (whole.len() <= zeros || !whole.ends_with(&"0".repeat(zeros))))
        {
            return Err("decimal_scale_loss".into());
        }
        let reduced = if whole == "0" {
            "0"
        } else {
            &whole[..whole.len() - zeros]
        };
        return coefficient(
            &format!("{}{reduced}", if negative { "-" } else { "" }),
            precision,
            0,
        );
    }
    if frac.len() > scale as usize {
        return Err("decimal_scale_loss".into());
    }
    let digits = format!("{whole}{frac}{}", "0".repeat(scale as usize - frac.len()));
    if digits.trim_start_matches('0').len() > precision as usize {
        return Err("decimal_precision_loss".into());
    }
    let n = digits.parse::<i128>().map_err(|_| "decimal_overflow")?;
    Ok(if negative { -n } else { n })
}

pub(super) fn unhex(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("raw_hex".into());
    }
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(|_| "raw_hex".into()))
        .collect()
}

fn batch(part: &Value) -> Result<(RecordBatch, Value)> {
    let rows = part["rows"].as_array().ok_or("rows")?;
    if rows.len() > 4096 {
        return Err("arrow_row_budget".into());
    }
    let columns = part["table"]["columns"].as_array().ok_or("columns")?;
    if columns.is_empty()
        || columns.len() > 256
        || rows
            .iter()
            .any(|r| r.as_array().map(|r| r.len()) != Some(columns.len()))
    {
        return Err("arrow_row_shape".into());
    }
    let mut fields = Vec::new();
    let mut arrays: Vec<ArrayRef> = Vec::new();
    let mut declarations = Vec::new();
    let mut mappings = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for (i, column) in columns.iter().enumerate() {
        let name = text(column, "name")?;
        if matches!(name.as_str(), "_oo_pos" | "_oo_op") || !names.insert(name.clone()) {
            return Err("reserved_arrow_field".into());
        }
        let kind = text(column, "type")?;
        let nullable = column["nullable"] == "Y";
        let values = rows
            .iter()
            .map(|r| match &r[i] {
                Value::Null if nullable => Ok(None),
                Value::String(s) => Ok(Some(s.as_str())),
                _ => Err("arrow_cell_shape".into()),
            })
            .collect::<Result<Vec<_>>>()?;
        let (data_type, array, token, p, s): (DataType, ArrayRef, &str, Option<i32>, Option<i32>) =
            match kind.as_str() {
                "NUMBER" => {
                    let p = column["precision"]
                        .as_u64()
                        .filter(|n| (1..=38).contains(n))
                        .ok_or("unconstrained_number_target_codec_required")?
                        as u8;
                    let s = column["scale"]
                        .as_i64()
                        .filter(|n| *n >= 0 && *n <= p as i64)
                        .ok_or("negative_or_unknown_scale_target_codec_required")?
                        as i8;
                    if s == 0 && p <= 18 {
                        let data = values
                            .iter()
                            .map(|v| {
                                v.map(|v| {
                                    i64::try_from(coefficient(v, p, 0)?)
                                        .map_err(|_| "int64_loss".into())
                                })
                                .transpose()
                            })
                            .collect::<Result<Vec<_>>>()?;
                        (
                            DataType::Int64,
                            Arc::new(Int64Array::from(data)),
                            "int64",
                            Some(64),
                            None,
                        )
                    } else {
                        let data = values
                            .iter()
                            .map(|v| v.map(|v| coefficient(v, p, s)).transpose())
                            .collect::<Result<Vec<_>>>()?;
                        let array = Decimal128Array::from(data)
                            .with_precision_and_scale(p, s)
                            .map_err(|_| "decimal_schema")?;
                        (
                            DataType::Decimal128(p, s),
                            Arc::new(array),
                            "decimal128",
                            Some(p as i32),
                            Some(s as i32),
                        )
                    }
                }
                "RAW" => {
                    let data = values
                        .iter()
                        .map(|v| v.map(unhex).transpose())
                        .collect::<Result<Vec<_>>>()?;
                    let array = BinaryArray::from_iter(data.iter().map(|v| v.as_deref()));
                    (DataType::Binary, Arc::new(array), "binary", None, None)
                }
                "VARCHAR2" | "NVARCHAR2" | "CHAR" | "NCHAR" => (
                    DataType::Utf8,
                    Arc::new(StringArray::from(values)),
                    "utf8",
                    None,
                    None,
                ),
                "DATE" => {
                    let data = values
                        .iter()
                        .map(|v| v.map(|v| super::temporal::micros(v, &kind)).transpose())
                        .collect::<Result<Vec<_>>>()?;
                    (
                        DataType::Timestamp(TimeUnit::Microsecond, None),
                        Arc::new(TimestampMicrosecondArray::from(data)),
                        "timestamp_micros",
                        None,
                        None,
                    )
                }
                _ if kind.starts_with("TIMESTAMP(") => {
                    super::temporal::timestamp_precision(&kind)?;
                    let zoned = kind.ends_with(" WITH TIME ZONE");
                    if zoned
                        && part
                            .pointer("/temporal_policy/timezone")
                            .and_then(Value::as_str)
                            != Some("utc_instant")
                    {
                        return Err("timezone_normalization_policy_required".into());
                    }
                    let data = values
                        .iter()
                        .map(|v| v.map(|v| super::temporal::micros(v, &kind)).transpose())
                        .collect::<Result<Vec<_>>>()?;
                    let array = TimestampMicrosecondArray::from(data);
                    if zoned {
                        (
                            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                            Arc::new(array.with_timezone("UTC")),
                            "timestamp_micros_utc",
                            None,
                            None,
                        )
                    } else {
                        (
                            DataType::Timestamp(TimeUnit::Microsecond, None),
                            Arc::new(array),
                            "timestamp_micros",
                            None,
                            None,
                        )
                    }
                }
                _ => return Err(format!("typed_arrow_target_mapping_required:{kind}")),
            };
        fields.push(Field::new(&name, data_type, nullable));
        arrays.push(array);
        declarations.push(
            json!({"name":name,"logical_name":name,"arrow_type":token,"nullable":nullable,
            "precision":p,"scale":s,"char_length":null,"document":false}),
        );
        mappings.push(json!({"name":name,"source_column":column,"arrow_type":token,
            "temporal_semantics":if kind=="DATE" || token=="timestamp_micros" {
                "civil time without timezone; all submicroseconds must be zero"
            } else if token=="timestamp_micros_utc" {
                "explicit utc_instant policy; exact instant; original region/offset representation not retained"
            } else {"not_temporal"}}));
    }
    let keys = part["table"]["keys"]
        .as_array()
        .ok_or("key_columns")?
        .iter()
        .map(|name| {
            let i = columns
                .iter()
                .position(|c| c["name"] == *name)
                .ok_or("key_column")?;
            if columns[i]["nullable"] != "N" {
                return Err("nullable_key".into());
            }
            Ok(i)
        })
        .collect::<Result<Vec<_>>>()?;
    if keys.is_empty() {
        return Err("key_columns".into());
    }
    let target_columns = fields
        .iter()
        .map(|f| {
            let (oid, typmod) = match f.data_type() {
                DataType::Int64 => (20, -1),
                DataType::Decimal128(p, s) => (1700, 4 + ((*p as i32) << 16) + *s as i32),
                DataType::Utf8 => (25, -1),
                DataType::Binary => (17, -1),
                DataType::Timestamp(TimeUnit::Microsecond, None) => (1114, -1),
                DataType::Timestamp(TimeUnit::Microsecond, Some(_)) => (1184, -1),
                _ => return Err("target_type".to_string()),
            };
            Ok(json!({"name":f.name(),"oid":oid,"typmod":typmod,"nullable":f.is_nullable()}))
        })
        .collect::<Result<Vec<_>>>()?;
    if let Some(cdc) = part.get("cdc") {
        let positions = cdc["positions"]
            .as_array()
            .filter(|v| v.len() == rows.len())
            .ok_or("cdc_positions")?;
        let ops = cdc["operations"]
            .as_array()
            .filter(|v| v.len() == rows.len())
            .ok_or("cdc_operations")?;
        let positions = positions
            .iter()
            .map(|p| {
                let s = p.as_str().ok_or("cdc_position")?;
                if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("cdc_position".into());
                }
                coefficient(s, 38, 0)
            })
            .collect::<Result<Vec<_>>>()?;
        let ops = ops
            .iter()
            .map(|op| {
                op.as_str()
                    .filter(|op| matches!(*op, "upsert" | "delete"))
                    .ok_or("cdc_operation")
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        fields.push(Field::new("_oo_pos", DataType::Decimal128(38, 0), false));
        fields.push(Field::new("_oo_op", DataType::Utf8, false));
        arrays.push(Arc::new(
            Decimal128Array::from(positions)
                .with_precision_and_scale(38, 0)
                .map_err(|_| "cdc_schema")?,
        ));
        arrays.push(Arc::new(StringArray::from(ops)));
    }
    let schema = Arc::new(Schema::new(fields));
    let batch = RecordBatch::try_new(schema.clone(), arrays).map_err(|_| "arrow_batch")?;
    Ok((
        batch,
        json!({"columns":declarations,"source_mappings":mappings,
        "typed_spec":{"columns":target_columns,"keys":keys},"cdc":part.get("cdc").is_some()}),
    ))
}

pub fn encode(part: &Value, output: &Path) -> Result<Value> {
    if output.exists() {
        return Err("arrow_output_exists".into());
    }
    let (batch, descriptor) = batch(part)?;
    let schema = batch.schema();
    let temp = output.with_extension("arrow.partial");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|_| "arrow_create")?;
    let mut writer = StreamWriter::try_new(file, &schema).map_err(|_| "arrow_schema")?;
    writer.write(&batch).map_err(|_| "arrow_write")?;
    writer.finish().map_err(|_| "arrow_finish")?;
    writer
        .into_inner()
        .map_err(|_| "arrow_finish")?
        .sync_all()
        .map_err(|_| "arrow_sync")?;
    if fs::metadata(&temp).map_err(|_| "arrow_stat")?.len() > 32 * 1024 * 1024 {
        return Err("arrow_byte_budget".into());
    }
    let mut reader = StreamReader::try_new(fs::File::open(&temp).map_err(|_| "arrow_open")?, None)
        .map_err(|_| "arrow_reopen")?;
    if reader
        .next()
        .ok_or("arrow_empty")?
        .map_err(|_| "arrow_decode")?
        != batch
        || reader.next().is_some()
    {
        return Err("arrow_typed_roundtrip".into());
    }
    fs::rename(&temp, output).map_err(|_| "arrow_rename")?;
    if let Some(parent) = output.parent() {
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| "arrow_dir_sync")?;
    }
    let bytes = fs::read(output).map_err(|_| "arrow_read")?;
    Ok(
        json!({"protocol":"oo.oracle.typed-arrow-part/v1","source_engine":"oracle",
        "rows":batch.num_rows(),"bytes":bytes.len(),"arrow_sha256":digest(&bytes),"columns":descriptor["columns"],
        "typed_spec":descriptor["typed_spec"],"source_mappings":descriptor["source_mappings"],
        "format":"Arrow IPC stream","typed_roundtrip":true,"cdc":descriptor["cdc"],
        "target_apply":false,"admitted":false}),
    )
}

fn read_input(entry: &Value) -> Result<Value> {
    let path = text(entry, "path")?;
    let path = Path::new(&path);
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("part_path".into());
    }
    let meta = fs::symlink_metadata(path).map_err(|_| "part_stat")?;
    if !meta.is_file() || meta.len() > 32 * 1024 * 1024 {
        return Err("part_budget".into());
    }
    let bytes = fs::read(path).map_err(|_| "part_read")?;
    if digest(&bytes) != text(entry, "sha256")? {
        return Err("part_digest".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "part_json".into())
}

/// One transaction/table file, bounded record batches. Never split a commit.
pub fn encode_series(entries: &[Value], output: &Path) -> Result<Value> {
    if entries.is_empty() || entries.len() > 1024 || output.exists() {
        return Err("arrow_series_budget".into());
    }
    let temp = output.with_extension("arrow.partial");
    let mut writer: Option<StreamWriter<fs::File>> = None;
    let mut descriptor = Value::Null;
    let mut total = 0;
    for entry in entries {
        let (record, current) = batch(&read_input(entry)?)?;
        total += record.num_rows();
        if total > 100_000 {
            return Err("transaction_row_budget".into());
        }
        if let Some(writer) = writer.as_mut() {
            if descriptor != current {
                return Err("series_schema_drift".into());
            }
            writer.write(&record).map_err(|_| "arrow_write")?;
        } else {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|_| "arrow_create")?;
            let mut first =
                StreamWriter::try_new(file, &record.schema()).map_err(|_| "arrow_schema")?;
            first.write(&record).map_err(|_| "arrow_write")?;
            writer = Some(first);
            descriptor = current;
        }
        if fs::metadata(&temp).map_err(|_| "arrow_stat")?.len() > 32 * 1024 * 1024 {
            return Err("transaction_byte_budget".into());
        }
    }
    let mut writer = writer.ok_or("arrow_empty")?;
    writer.finish().map_err(|_| "arrow_finish")?;
    writer
        .into_inner()
        .map_err(|_| "arrow_finish")?
        .sync_all()
        .map_err(|_| "arrow_sync")?;
    if fs::metadata(&temp).map_err(|_| "arrow_stat")?.len() > 32 * 1024 * 1024 {
        return Err("transaction_byte_budget".into());
    }
    let mut decoded = StreamReader::try_new(fs::File::open(&temp).map_err(|_| "arrow_read")?, None)
        .map_err(|_| "arrow_decode")?;
    for entry in entries {
        let expected = batch(&read_input(entry)?)?.0;
        if decoded
            .next()
            .ok_or("arrow_missing_batch")?
            .map_err(|_| "arrow_decode")?
            != expected
        {
            return Err("arrow_typed_roundtrip".into());
        }
    }
    if decoded.next().is_some() {
        return Err("arrow_extra_batch".into());
    }
    fs::rename(&temp, output).map_err(|_| "arrow_rename")?;
    if let Some(parent) = output.parent() {
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| "arrow_dir_sync")?;
    }
    let bytes = fs::read(output).map_err(|_| "arrow_read")?;
    Ok(
        json!({"protocol":"oo.oracle.typed-arrow-part/v1","source_engine":"oracle","rows":total,
        "record_batches":entries.len(),"bytes":bytes.len(),"arrow_sha256":digest(&bytes),
        "columns":descriptor["columns"],"typed_spec":descriptor["typed_spec"],
        "source_mappings":descriptor["source_mappings"],"cdc":descriptor["cdc"],
        "format":"Arrow IPC stream","typed_roundtrip":true,"target_apply":false,"admitted":false}),
    )
}

/// Small independent IPC inspection for integration assertions, not an unbounded dump.
pub fn inspect(path: &Path, expected: &str, max_rows: usize) -> Result<Value> {
    if !(1..=64).contains(&max_rows)
        || fs::metadata(path).map_err(|_| "arrow_stat")?.len() > 32 * 1024 * 1024
    {
        return Err("inspect_budget".into());
    }
    let bytes = fs::read(path).map_err(|_| "arrow_read")?;
    if digest(&bytes) != expected {
        return Err("arrow_digest".into());
    }
    let reader =
        StreamReader::try_new(std::io::Cursor::new(bytes), None).map_err(|_| "arrow_decode")?;
    let mut rows = Vec::new();
    for record in reader {
        let record = record.map_err(|_| "arrow_decode")?;
        if rows.len() + record.num_rows() > max_rows {
            return Err("inspect_row_budget".into());
        }
        for i in 0..record.num_rows() {
            let mut row = Vec::new();
            for column in record.columns() {
                let value = if column.is_null(i) {
                    Value::Null
                } else {
                    match column.data_type() {
                        DataType::Int64 => json!(
                            column
                                .as_any()
                                .downcast_ref::<Int64Array>()
                                .ok_or("inspect_type")?
                                .value(i)
                                .to_string()
                        ),
                        DataType::Decimal128(_, scale) => {
                            json!({"coefficient":column.as_any().downcast_ref::<Decimal128Array>().ok_or("inspect_type")?.value(i).to_string(),"scale":scale})
                        }
                        DataType::Utf8 => json!(
                            column
                                .as_any()
                                .downcast_ref::<StringArray>()
                                .ok_or("inspect_type")?
                                .value(i)
                        ),
                        DataType::Binary => json!(super::chunk::hex(
                            column
                                .as_any()
                                .downcast_ref::<BinaryArray>()
                                .ok_or("inspect_type")?
                                .value(i)
                        )),
                        DataType::Timestamp(TimeUnit::Microsecond, zone) => {
                            json!({"micros":column.as_any().downcast_ref::<TimestampMicrosecondArray>().ok_or("inspect_type")?.value(i).to_string(),"timezone":zone.as_deref()})
                        }
                        _ => return Err("inspect_type".into()),
                    }
                };
                row.push(value);
            }
            rows.push(row);
        }
    }
    Ok(json!({"rows":rows,"truncated":false}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decimal_coefficient_is_exact() {
        assert_eq!(
            coefficient("99999999999999999999.123456789", 38, 9).unwrap(),
            99999999999999999999123456789
        );
        assert_eq!(coefficient("-0.000000001", 38, 9).unwrap(), -1);
        assert!(coefficient("0.0000000001", 38, 9).is_err());
        assert!(coefficient("1234", 3, 0).is_err());
    }
    #[test]
    fn actual_ipc_native_types_and_cdc_suffix_roundtrip() {
        let dir = std::env::temp_dir().join(format!("oo-oracle-arrow-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let output = dir.join("part.arrow");
        let part = json!({"table":{"columns":[
            {"name":"ID","type":"NUMBER","precision":9,"scale":0,"nullable":"N"},
            {"name":"AMOUNT","type":"NUMBER","precision":38,"scale":9,"nullable":"Y"},
            {"name":"PAYLOAD","type":"RAW","nullable":"Y"}],"keys":["ID"]},
            "rows":[["1","99999999999999999999.123456789","0000ff"],["2",null,null]],
            "cdc":{"positions":["1","1"],"operations":["upsert","delete"]}});
        let receipt = encode(&part, &output).unwrap();
        assert_eq!(receipt["rows"], 2);
        assert_eq!(receipt["columns"][1]["arrow_type"], "decimal128");
        assert!(encode(&part, &output).is_err());
        fs::remove_file(output).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn temporal_schema_preserves_time_and_requires_explicit_zone_policy() {
        let mut part = json!({"table":{"keys":["ID"],"columns":[
            {"name":"ID","type":"NUMBER","precision":9,"scale":0,"nullable":"N"},
            {"name":"D","type":"DATE","nullable":"Y"},
            {"name":"TS","type":"TIMESTAMP(9)","nullable":"Y"},
            {"name":"TZ","type":"TIMESTAMP(6) WITH TIME ZONE","nullable":"Y"}]},
            "rows":[["1","1970-01-01T01:23:45","1970-01-01T01:23:45.123456000","1970-01-01T01:23:45.123456000 -00:30"]]});
        assert_eq!(
            batch(&part).unwrap_err(),
            "timezone_normalization_policy_required"
        );
        part["temporal_policy"] = json!({"timezone":"utc_instant"});
        let (_, spec) = batch(&part).unwrap();
        assert_eq!(spec["typed_spec"]["columns"][1]["oid"], 1114);
        assert_eq!(spec["typed_spec"]["columns"][3]["oid"], 1184);
        part["rows"][0][2] = json!("1970-01-01T01:23:45.123456001");
        assert_eq!(
            batch(&part).unwrap_err(),
            "submicrosecond_target_contract_required"
        );
    }
}
