//! Genuine Arrow IPC stream writer. Values are native arrays, not a UTF-8 row dump.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use arrow_array::builder::{
    BinaryBuilder, BooleanBuilder, Float64Builder, Int64Builder, ListBuilder, StringBuilder,
    TimestampMicrosecondBuilder,
};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{DataType, Field, Schema, TimeUnit};

use crate::cast::{Cell, FieldPlan};
use crate::catalog::EncodedRow;
use crate::PROTOCOL;

pub fn write_ipc(plan_fields: &[FieldPlan], rows: &[EncodedRow], schema_meta: BTreeMap<String, String>) -> Result<Vec<u8>, String> {
    let schema = schema_for(plan_fields, schema_meta)?;
    let batch = batch_for(&schema, plan_fields, rows)?;
    let mut bytes = Vec::new();
    {
        let mut writer = StreamWriter::try_new(&mut bytes, &schema).map_err(|error| format!("arrow_writer:{error}"))?;
        writer.write(&batch).map_err(|error| format!("arrow_write:{error}"))?;
        writer.finish().map_err(|error| format!("arrow_finish:{error}"))?;
    }
    if bytes.len() < 8
        || bytes[..4] != [0xff, 0xff, 0xff, 0xff]
        || bytes[bytes.len() - 8..] != [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]
    {
        return Err("arrow_stream_eos_missing".into());
    }
    Ok(bytes)
}

fn schema_for(fields: &[FieldPlan], mut meta: BTreeMap<String, String>) -> Result<Schema, String> {
    meta.insert("oo.protocol".into(), PROTOCOL.to_string());
    let mut arrow_fields = vec![
        field("source_doc_id", DataType::Utf8, false, &[("oo.field_role", "identity")]),
        field("source_index", DataType::Utf8, false, &[("oo.field_role", "identity")]),
        field("raw_source", DataType::Binary, false, &[("oo.field_role", "raw_document_provenance")]),
    ];
    for plan in fields {
        if plan.unsupported {
            continue;
        }
        let dtype = arrow_type(plan)?;
        let mut pairs: Vec<(String, String)> = plan.metadata.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        pairs.push(("oo.field_role".into(), "value".into()));
        arrow_fields.push(Field::new(&plan.name, dtype, true).with_metadata(to_map(&pairs)));
        arrow_fields.push(
            Field::new(format!("present__{}", plan.name), DataType::Boolean, false).with_metadata(to_map(&[(
                "oo.field_role".into(),
                "presence".into(),
            )])),
        );
    }
    Ok(Schema::new_with_metadata(arrow_fields, to_map_owned(meta)))
}

fn field(name: &str, dtype: DataType, nullable: bool, meta: &[(&str, &str)]) -> Field {
    let pairs = meta
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect::<Vec<_>>();
    Field::new(name, dtype, nullable).with_metadata(to_map(&pairs))
}

fn to_map(pairs: &[(String, String)]) -> HashMap<String, String> {
    pairs.iter().cloned().collect()
}

fn to_map_owned(pairs: BTreeMap<String, String>) -> HashMap<String, String> {
    pairs.into_iter().collect()
}

fn arrow_type(plan: &FieldPlan) -> Result<DataType, String> {
    let multi = plan.metadata.get("oo.multi").map(String::as_str) == Some("true");
    let scalar = match plan.os_type.as_str() {
        "keyword" | "text" => DataType::Utf8,
        "long" => DataType::Int64,
        "double" => DataType::Float64,
        "scaled_float" => DataType::Int64,
        "boolean" => DataType::Boolean,
        "binary" => DataType::Binary,
        "date" => DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
        "object" if plan.json_text => DataType::Utf8,
        "object" => return Err(format!("object_requires_original_json_text:{}", plan.name)),
        other => return Err(format!("no_arrow_type:{other}")),
    };
    if multi && plan.os_type == "long" {
        return Ok(DataType::Utf8);
    }
    if multi && (plan.os_type == "keyword" || plan.os_type == "text") {
        return Ok(DataType::List(Arc::new(Field::new("item", DataType::Binary, true))));
    }
    if multi {
        Ok(DataType::List(Arc::new(Field::new("item", scalar, true))))
    } else {
        Ok(scalar)
    }
}

fn batch_for(schema: &Schema, plans: &[FieldPlan], rows: &[EncodedRow]) -> Result<RecordBatch, String> {
    let mut columns: Vec<ArrayRef> = Vec::new();
    columns.push(strings(rows.iter().map(|row| Some(row.source_doc_id.as_str()))));
    columns.push(strings(rows.iter().map(|row| Some(row.source_index.as_str()))));
    columns.push(binaries(rows.iter().map(|row| Some(row.raw_source.as_slice()))));
    for plan in plans {
        if plan.unsupported {
            continue;
        }
        let mut present = BooleanBuilder::new();
        let array = build_column(plan, rows, &mut present)?;
        columns.push(array);
        columns.push(Arc::new(present.finish()));
    }
    RecordBatch::try_new(Arc::new(schema.clone()), columns).map_err(|error| format!("record_batch:{error}"))
}

fn build_column(plan: &FieldPlan, rows: &[EncodedRow], present: &mut BooleanBuilder) -> Result<ArrayRef, String> {
    let multi = plan.metadata.get("oo.multi").map(String::as_str) == Some("true");
    if multi && plan.os_type == "long" {
        return build_json_text(plan, rows, present);
    }
    if multi {
        return build_list(plan, rows, present);
    }
    if plan.json_text || plan.os_type == "keyword" || plan.os_type == "text" {
        let mut builder = StringBuilder::new();
        for row in rows {
            let (flag, cell) = cell_of(row, &plan.name)?;
            present.append_value(flag);
            match cell {
                Cell::Null => builder.append_null(),
                Cell::Text(value) | Cell::Json(value) => builder.append_value(value),
                _ => return Err(format!("cell_type:{}", plan.name)),
            }
        }
        return Ok(Arc::new(builder.finish()));
    }
    match plan.os_type.as_str() {
        "long" | "scaled_float" => {
            let mut builder = Int64Builder::new();
            for row in rows {
                let (flag, cell) = cell_of(row, &plan.name)?;
                present.append_value(flag);
                match cell {
                    Cell::Null => builder.append_null(),
                    Cell::I64(value) => builder.append_value(*value),
                    _ => return Err(format!("cell_type:{}", plan.name)),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        "double" => {
            let mut builder = Float64Builder::new();
            for row in rows {
                let (flag, cell) = cell_of(row, &plan.name)?;
                present.append_value(flag);
                match cell {
                    Cell::Null => builder.append_null(),
                    Cell::F64(value) => builder.append_value(*value),
                    _ => return Err(format!("cell_type:{}", plan.name)),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        "boolean" => {
            let mut builder = BooleanBuilder::new();
            for row in rows {
                let (flag, cell) = cell_of(row, &plan.name)?;
                present.append_value(flag);
                match cell {
                    Cell::Null => builder.append_null(),
                    Cell::Bool(value) => builder.append_value(*value),
                    _ => return Err(format!("cell_type:{}", plan.name)),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        "binary" => {
            let mut builder = BinaryBuilder::new();
            for row in rows {
                let (flag, cell) = cell_of(row, &plan.name)?;
                present.append_value(flag);
                match cell {
                    Cell::Null => builder.append_null(),
                    Cell::Bytes(value) => builder.append_value(value),
                    _ => return Err(format!("cell_type:{}", plan.name)),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        "date" => {
            let mut builder = TimestampMicrosecondBuilder::new().with_timezone("UTC");
            for row in rows {
                let (flag, cell) = cell_of(row, &plan.name)?;
                present.append_value(flag);
                match cell {
                    Cell::Null => builder.append_null(),
                    Cell::TimestampMillis(value) => {
                        let micros = value.checked_mul(1_000).ok_or_else(|| format!("millis_overflow:{}", plan.name))?;
                        builder.append_value(micros);
                    }
                    _ => return Err(format!("cell_type:{}", plan.name)),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        "object" => Err(format!("object_requires_original_json_text:{}", plan.name)),
        other => Err(format!("column_type:{other}")),
    }
}

fn build_json_text(plan: &FieldPlan, rows: &[EncodedRow], present: &mut BooleanBuilder) -> Result<ArrayRef, String> {
    let mut builder = StringBuilder::new();
    for row in rows {
        let (flag, cell) = cell_of(row, &plan.name)?;
        present.append_value(flag);
        match cell {
            Cell::Null => builder.append_null(),
            Cell::Json(value) => builder.append_value(value),
            _ => return Err(format!("cell_type:{}", plan.name)),
        }
    }
    Ok(Arc::new(builder.finish()))
}

fn build_list(plan: &FieldPlan, rows: &[EncodedRow], present: &mut BooleanBuilder) -> Result<ArrayRef, String> {
    match plan.os_type.as_str() {
        "keyword" | "text" => {
            let item = Arc::new(Field::new("item", DataType::Binary, true));
            let mut builder = ListBuilder::new(BinaryBuilder::new()).with_field(item);
            for row in rows {
                let (flag, cell) = cell_of(row, &plan.name)?;
                present.append_value(flag);
                match cell {
                    Cell::Null => builder.append(false),
                    Cell::ListText(values) => {
                        for value in values {
                            match value {
                                Some(text) => builder.values().append_value(text.as_bytes()),
                                None => builder.values().append_null(),
                            }
                        }
                        builder.append(true);
                    }
                    _ => return Err(format!("cell_type:{}", plan.name)),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        other => Err(format!("list_column:{other}")),
    }
}

fn cell_of<'a>(row: &'a EncodedRow, name: &str) -> Result<(bool, &'a Cell), String> {
    row.values
        .get(name)
        .map(|(flag, cell)| (*flag, cell))
        .ok_or_else(|| format!("missing_cell:{name}"))
}

fn strings<'a>(values: impl Iterator<Item = Option<&'a str>>) -> ArrayRef {
    let mut builder = StringBuilder::new();
    for value in values {
        match value {
            Some(text) => builder.append_value(text),
            None => builder.append_null(),
        }
    }
    Arc::new(builder.finish())
}

fn binaries<'a>(values: impl Iterator<Item = Option<&'a [u8]>>) -> ArrayRef {
    let mut builder = BinaryBuilder::new();
    for value in values {
        match value {
            Some(bytes) => builder.append_value(bytes),
            None => builder.append_null(),
        }
    }
    Arc::new(builder.finish())
}
