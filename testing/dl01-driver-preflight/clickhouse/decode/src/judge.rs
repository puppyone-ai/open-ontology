use std::fs;
use std::path::Path;

use arrow_schema::{DataType, Field, Schema, TimeUnit};
use serde_json::{json, Value};

use crate::extract::{self, load_ipc};
use crate::fail::{fail, Gate};

const READINGS: [&str; 12] = [
    "reading_id",
    "version",
    "amount",
    "note",
    "code",
    "code_or_null",
    "ts",
    "tags",
    "labels",
    "qty",
    "label",
    "wide_u64",
];

pub fn compare_readings(ipc_path: &Path, gold_path: &Path) -> Gate<Value> {
    let bytes = fs::read(ipc_path).map_err(|error| fail(format!("read ipc: {error}")))?;
    let gold = read_json(gold_path)?;
    let loaded = match load_ipc(&bytes) {
        Ok(loaded) => loaded,
        Err(error) => {
            return Ok(json!({
                "passed": false,
                "failures": [error.0],
                "rows": [],
            }));
        }
    };
    let mut failures = readings_type_failures(loaded.schema.as_ref());
    let mut rows = Vec::new();
    for values in &loaded.rows {
        if values.len() != loaded.schema.fields().len() {
            failures.push("row width does not match the schema".into());
            continue;
        }
        let mut object = serde_json::Map::new();
        for (field, value) in loaded.schema.fields().iter().zip(values) {
            object.insert(field.name().clone(), value.clone());
        }
        rows.push(Value::Object(object));
    }
    match gold.as_array() {
        Some(expected) if rows == *expected && failures.is_empty() => {}
        Some(expected) => {
            if rows != *expected {
                let got = rows.len();
                let want = expected.len();
                failures.push(format!("rows != gold (got {got} rows, gold {want})"));
            }
        }
        None => failures.push("gold is not an array".into()),
    }
    Ok(json!({
        "passed": failures.is_empty(),
        "failures": failures,
        "field_types": field_types(loaded.schema.as_ref()),
        "rows": rows,
    }))
}

pub fn readings_type_failures(schema: &Schema) -> Vec<String> {
    let mut failures = Vec::new();
    for name in READINGS {
        match schema.column_with_name(name) {
            Some((_, field)) => {
                if let Some(reason) = column_problem(name, field) {
                    failures.push(format!("{name}: {reason}"));
                }
            }
            None => failures.push(format!("missing column {name}")),
        }
    }
    failures
}

fn column_problem(name: &str, field: &Field) -> Option<String> {
    let dtype = field.data_type();
    if let Some(reason) = unsafe_type(dtype) {
        return Some(reason.into());
    }
    let ok = match name {
        "reading_id" | "version" | "wide_u64" => matches!(dtype, DataType::UInt64),
        "qty" => matches!(dtype, DataType::Int64),
        "amount" => matches!(dtype, DataType::Decimal256(76, 10)),
        "ts" => match dtype {
            DataType::Timestamp(TimeUnit::Nanosecond, tz) => extract::tz_is_utc(tz),
            _ => false,
        },
        "note" | "code" | "code_or_null" | "label" => is_text(dtype),
        "tags" => is_list_of(dtype, DataType::Int64),
        "labels" => is_text_list(dtype),
        _ => false,
    };
    if ok {
        None
    } else {
        Some(format!("type {dtype} is not the native oracle for {name}"))
    }
}

fn unsafe_type(dtype: &DataType) -> Option<&'static str> {
    match dtype {
        DataType::Float16 | DataType::Float32 | DataType::Float64 => {
            Some("float is not an exact oracle")
        }
        DataType::Decimal128(_, _) => Some("Decimal128 cannot hold Decimal(76, 10)"),
        DataType::Timestamp(unit, _) if *unit != TimeUnit::Nanosecond => {
            Some("coarser timestamp is precision_loss against DateTime64(9)")
        }
        DataType::Dictionary(_, inner) => unsafe_type(inner),
        DataType::List(field)
        | DataType::LargeList(field)
        | DataType::ListView(field)
        | DataType::LargeListView(field)
        | DataType::FixedSizeList(field, _) => unsafe_type(field.data_type()),
        _ => None,
    }
}

fn is_text(dtype: &DataType) -> bool {
    match dtype {
        DataType::Utf8
        | DataType::LargeUtf8
        | DataType::Utf8View
        | DataType::Binary
        | DataType::LargeBinary
        | DataType::BinaryView => true,
        DataType::Dictionary(_, inner) => is_text(inner),
        _ => false,
    }
}

fn is_list_of(dtype: &DataType, want: DataType) -> bool {
    match dtype {
        DataType::List(field)
        | DataType::LargeList(field)
        | DataType::ListView(field)
        | DataType::LargeListView(field)
        | DataType::FixedSizeList(field, _) => field.data_type() == &want,
        _ => false,
    }
}

fn is_text_list(dtype: &DataType) -> bool {
    match dtype {
        DataType::List(field)
        | DataType::LargeList(field)
        | DataType::ListView(field)
        | DataType::LargeListView(field)
        | DataType::FixedSizeList(field, _) => is_text(field.data_type()),
        _ => false,
    }
}

pub fn classify_bytes(ipc_path: &Path, column: &str, expect_hex: &str, utf8_constraint: bool) -> Gate<Value> {
    let bytes = fs::read(ipc_path).map_err(|error| fail(format!("read ipc: {error}")))?;
    let expect = decode_hex(expect_hex)?;
    // Byte probes stay on the stream batch. The text loader refuses non-UTF-8
    // binary, which is correct for gold rows and wrong for this comparison.
    let batch = match load_stream_batch(&bytes) {
        Ok(batch) => batch,
        Err(error) => {
            return Ok(json!({
                "passed": utf8_constraint,
                "classification": if utf8_constraint { "decoder_rejected" } else { "unreadable" },
                "unsafe_substitution": false,
                "detail": error.0,
            }));
        }
    };
    let Some(position) = batch.schema().index_of(column).ok() else {
        return Ok(json!({
            "passed": false,
            "classification": "missing_column",
            "unsafe_substitution": false,
            "detail": column,
        }));
    };
    if batch.num_rows() != 1 {
        return Ok(json!({
            "passed": false,
            "classification": "row_count",
            "unsafe_substitution": false,
            "detail": batch.num_rows(),
        }));
    }
    let raw = match extract::raw_bytes(batch.column(position).as_ref(), 0) {
        Ok(raw) => raw,
        Err(error) => {
            return Ok(json!({
                "passed": utf8_constraint,
                "classification": if utf8_constraint { "decoder_rejected" } else { "unreadable" },
                "unsafe_substitution": false,
                "detail": error.0,
            }));
        }
    };
    if raw == expect {
        return Ok(json!({
            "passed": true,
            "classification": "binary_preserved",
            "unsafe_substitution": false,
            "detail": expect_hex,
        }));
    }
    let substitution = std::str::from_utf8(&raw).is_ok();
    Ok(json!({
        "passed": false,
        "classification": "substitution",
        "unsafe_substitution": substitution,
        "detail": hex_encode(&raw),
        "field_types": field_types(batch.schema().as_ref()),
    }))
}

fn load_stream_batch(bytes: &[u8]) -> Gate<arrow_array::RecordBatch> {
    use std::io::Cursor;

    use arrow_ipc::reader::StreamReader;
    let reader = StreamReader::try_new(Cursor::new(bytes), None)
        .map_err(|error| fail(format!("ipc schema is not a complete Arrow stream: {error}")))?;
    let mut only = None;
    for batch in reader {
        let batch = batch.map_err(|error| fail(format!("ipc batch is truncated or invalid: {error}")))?;
        if only.is_some() {
            return Err(fail("byte probe has more than one batch"));
        }
        only = Some(batch);
    }
    only.ok_or_else(|| fail("byte probe has no batch"))
}

pub fn judge_retirees(ipc_path: &Path, mutations_path: &Path) -> Gate<Value> {
    let bytes = fs::read(ipc_path).map_err(|error| fail(format!("read ipc: {error}")))?;
    let loaded = load_ipc(&bytes)?;
    let mut failures = Vec::new();
    let id_type = loaded.schema.field_with_name("id").ok();
    let payload_type = loaded.schema.field_with_name("payload").ok();
    if !id_type.is_some_and(|field| matches!(field.data_type(), DataType::UInt64)) {
        failures.push(format!("id type is not UInt64: {id_type:?}"));
    }
    if !payload_type.is_some_and(|field| is_text(field.data_type())) {
        failures.push(format!("payload type is not text: {payload_type:?}"));
    }
    let mut rows = Vec::new();
    for values in &loaded.rows {
        let mut object = serde_json::Map::new();
        for (field, value) in loaded.schema.fields().iter().zip(values) {
            object.insert(field.name().clone(), value.clone());
        }
        rows.push(Value::Object(object));
    }
    let has_9 = rows.iter().any(|row| row.get("id").and_then(Value::as_str) == Some("9"));
    let stays = rows.iter().any(|row| {
        row.get("id").and_then(Value::as_str) == Some("10")
            && row.get("payload").and_then(Value::as_str) == Some("stays")
    });
    if !stays {
        failures.push("id 10 payload stays is not visible".into());
    }
    let mutations_text = fs::read_to_string(mutations_path).map_err(|error| fail(format!("read mutations: {error}")))?;
    let mutations = json_lines(&mutations_text)?;
    if mutations.is_empty() {
        failures.push("system.mutations returned no retirees row".into());
    }
    let failed_reason = mutations.iter().any(|row| {
        row.get("latest_fail_reason")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty())
    });
    if failed_reason {
        failures.push("retirees mutation has latest_fail_reason".into());
    }
    let all_done = !mutations.is_empty()
        && mutations.iter().all(mutation_done);
    if has_9 && all_done {
        failures.push("finished mutation still shows retirees id 9".into());
    }
    let lag_observed = has_9 && !all_done;
    if !has_9 && rows.len() != 1 {
        let got = rows.len();
        failures.push(format!("visible retirees rows {got} after id 9 disappeared"));
    }
    Ok(json!({
        "passed": failures.is_empty(),
        "failures": failures,
        "lag_observed": lag_observed,
        "mutation_done": all_done,
        "rows": rows,
        "mutations": mutations,
        "field_types": field_types(loaded.schema.as_ref()),
    }))
}

fn mutation_done(row: &Value) -> bool {
    match row.get("is_done") {
        Some(Value::Number(number)) => number.as_u64() == Some(1),
        Some(Value::String(text)) => text == "1",
        Some(Value::Bool(value)) => *value,
        _ => false,
    }
}

fn field_types(schema: &Schema) -> Vec<Value> {
    schema
        .fields()
        .iter()
        .map(|field| {
            json!({
                "name": field.name(),
                "data_type": field.data_type().to_string(),
                "nullable": field.is_nullable(),
            })
        })
        .collect()
}

fn read_json(path: &Path) -> Gate<Value> {
    let shown = path.display().to_string();
    let text = fs::read_to_string(path).map_err(|error| fail(format!("read {shown}: {error}")))?;
    serde_json::from_str(&text).map_err(|error| fail(format!("json {shown}: {error}")))
}

fn json_lines(text: &str) -> Gate<Vec<Value>> {
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("Code:") {
            let snippet = &line[..line.len().min(300)];
            return Err(fail(format!("mutations body is an exception: {snippet}")));
        }
        let line_no = index + 1;
        rows.push(
            serde_json::from_str(line)
                .map_err(|error| fail(format!("mutations line {line_no} is not json: {error}")))?,
        );
    }
    Ok(rows)
}

fn decode_hex(text: &str) -> Gate<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(fail("expect-hex is not even hex"));
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let pair = std::str::from_utf8(&bytes[index..index + 2]).map_err(|_| fail("hex"))?;
        out.push(u8::from_str_radix(pair, 16).map_err(|_| fail("hex"))?);
        index += 2;
    }
    Ok(out)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_schema::{DataType, Field, Schema, TimeUnit};

    use super::readings_type_failures;

    fn field(name: &str, dtype: DataType) -> Field {
        Field::new(name, dtype, true)
    }

    fn native_schema() -> Schema {
        Schema::new(vec![
            field("reading_id", DataType::UInt64),
            field("version", DataType::UInt64),
            field("amount", DataType::Decimal256(76, 10)),
            field("note", DataType::Binary),
            field("code", DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Binary))),
            field("code_or_null", DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Binary))),
            field("ts", DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into()))),
            field("tags", DataType::List(Arc::new(Field::new("item", DataType::Int64, true)))),
            field("labels", DataType::List(Arc::new(Field::new("item", DataType::Binary, true)))),
            field("qty", DataType::Int64),
            field("label", DataType::Binary),
            field("wide_u64", DataType::UInt64),
        ])
    }

    #[test]
    fn native_readings_types_pass() {
        assert!(readings_type_failures(&native_schema()).is_empty());
    }

    #[test]
    fn utf8_string_columns_remain_exact() {
        let mut fields = native_schema().fields().iter().map(|field| field.as_ref().clone()).collect::<Vec<_>>();
        for field in &mut fields {
            let name = field.name().to_string();
            match name.as_str() {
                "note" | "code_or_null" | "label" => {
                    *field = Field::new(name, DataType::Utf8, true);
                }
                "code" => {
                    *field = Field::new(
                        name,
                        DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Utf8)),
                        true,
                    );
                }
                "labels" => {
                    *field = Field::new(
                        name,
                        DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
                        true,
                    );
                }
                _ => {}
            }
        }
        assert!(readings_type_failures(&Schema::new(fields)).is_empty());
    }

    #[test]
    fn float_decimal128_and_microseconds_are_rejected() {
        let mut fields = native_schema().fields().iter().map(|field| field.as_ref().clone()).collect::<Vec<_>>();
        for field in &mut fields {
            if field.name() == "wide_u64" {
                *field = Field::new("wide_u64", DataType::Float64, true);
            }
            if field.name() == "amount" {
                *field = Field::new("amount", DataType::Decimal128(38, 10), true);
            }
            if field.name() == "ts" {
                *field = Field::new("ts", DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())), true);
            }
        }
        let text = readings_type_failures(&Schema::new(fields)).join(" ");
        assert!(text.contains("float"), "{text}");
        assert!(text.contains("Decimal128"), "{text}");
        assert!(text.contains("precision_loss"), "{text}");
    }
}
