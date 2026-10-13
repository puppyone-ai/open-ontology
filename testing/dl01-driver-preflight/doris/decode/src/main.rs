//! Independent Arrow decode. It does not depend on the reader crate.

mod civil;
mod decimal;
mod fail;

use std::env;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::process::ExitCode;

use arrow_array::{Array, Date32Array, Decimal128Array, Int64Array, StringArray};
use arrow_ipc::reader::StreamReader;
use serde_json::{json, Value};

fn show(err: impl ToString) -> String {
    err.to_string()
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (Some(evidence), Some(oracle)) = (args.next(), args.next()) else {
        eprintln!("usage: doris-decode <evidence> <oracle>");
        return ExitCode::from(2);
    };
    match run(Path::new(&evidence), Path::new(&oracle)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn run(evidence: &Path, oracle_path: &Path) -> Result<(), String> {
    let oracle: Value = serde_json::from_slice(&fs::read(oracle_path).map_err(show)?)
        .map_err(show)?;
    let mut failures = Vec::new();
    let tables = oracle["tables"].as_object().ok_or("oracle tables")?;
    for (name, spec) in tables {
        if let Err(err) = check_table(evidence, name, spec) {
            failures.push(json!({"table": name, "error": err}));
        }
    }
    if let Err(err) = check_sum(evidence, &oracle) {
        failures.push(json!({"check": "sum", "error": err}));
    }
    if let Err(err) = check_order(evidence, &oracle) {
        failures.push(json!({"check": "string_order", "error": err}));
    }
    if let Err(err) = check_nulls(evidence, &oracle) {
        failures.push(json!({"check": "null_vs_zero", "error": err}));
    }
    if let Err(err) = check_caps(evidence) {
        failures.push(json!({"check": "caps", "error": err}));
    }
    if let Err(err) = check_saturation(evidence) {
        failures.push(json!({"check": "saturation", "error": err}));
    }
    let ready: Value = read_json(&evidence.join("ready.json")).unwrap_or(Value::Null);
    if ready["source_acceptance"] != false || ready["ready"] != false {
        failures.push(json!({"check": "ready", "error": "ready.json must record ready false and source_acceptance false"}));
    }
    let report = json!({
        "protocol": "oo.doris.decode/v0",
        "source_acceptance": false,
        "failures": failures,
        "ok": failures.is_empty()
    });
    write_json(&evidence.join("decode-report.json"), &report)?;
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} decode mismatches", failures.len()))
    }
}

fn check_table(evidence: &Path, name: &str, spec: &Value) -> Result<(), String> {
    let (batch, schema) = read_arrow(&evidence.join(format!("{name}.arrow")))?;
    let meta = schema.metadata();
    expect(meta.get("oo.doris.model").map(String::as_str), Some(spec["model"].as_str().unwrap_or("")), "model")?;
    expect(meta.get("oo.doris.final_keyword_required").map(String::as_str), Some("false"), "final")?;
    expect(meta.get("oo.doris.lossless").map(String::as_str), Some("true"), "lossless")?;
    let statement = meta.get("oo.doris.statement").map(String::as_str).unwrap_or("");
    if statement.to_ascii_uppercase().contains("FINAL") {
        return Err("statement contains FINAL".to_string());
    }
    let columns = spec["columns"].as_array().ok_or("columns")?;
    let rows = spec["rows"].as_array().ok_or("rows")?;
    if batch.num_rows() != rows.len() || batch.num_columns() != columns.len() {
        return Err(format!("shape {}x{} vs {}x{}", batch.num_rows(), batch.num_columns(), rows.len(), columns.len()));
    }
    for (row_index, expected) in rows.iter().enumerate() {
        let expected = expected.as_array().ok_or("row")?;
        for (col_index, column) in columns.iter().enumerate() {
            check_cell(batch.column(col_index), row_index, column, &expected[col_index])?;
        }
    }
    Ok(())
}

fn check_cell(array: &dyn Array, row: usize, column: &Value, expected: &Value) -> Result<(), String> {
    let kind = column["kind"].as_str().unwrap_or("");
    if expected.is_null() {
        return if array.is_null(row) { Ok(()) } else { Err(format!("{kind} row {row} expected null")) };
    }
    if array.is_null(row) {
        return Err(format!("{kind} row {row} unexpected null"));
    }
    match kind {
        "int64" => {
            let got = array.as_any().downcast_ref::<Int64Array>().ok_or("int")?.value(row);
            let want = expected.as_i64().ok_or("oracle int")?;
            if got == want { Ok(()) } else { Err(format!("int {got} != {want}")) }
        }
        "utf8" => {
            let got = array.as_any().downcast_ref::<StringArray>().ok_or("utf8")?.value(row);
            let want = expected.as_str().ok_or("oracle text")?;
            if got == want { Ok(()) } else { Err(format!("text {got:?} != {want:?}")) }
        }
        "date" => {
            let got = array.as_any().downcast_ref::<Date32Array>().ok_or("date")?.value(row);
            let want = civil::parse_date(expected.as_str().ok_or("oracle date")?).map_err(show)?;
            if got == want { Ok(()) } else { Err(format!("date {got} != {want}")) }
        }
        "decimal" => {
            let got = array.as_any().downcast_ref::<Decimal128Array>().ok_or("decimal")?.value(row);
            let precision = u8::try_from(column["precision"].as_u64().ok_or("precision")?).map_err(|_| "precision")?;
            let scale = u8::try_from(column["scale"].as_u64().ok_or("scale")?).map_err(|_| "scale")?;
            let want = decimal::parse_decimal(expected.as_str().ok_or("oracle decimal")?, precision, scale)
                .map_err(show)?;
            if got == want {
                Ok(())
            } else {
                let left = decimal::format_unscaled(got, scale).map_err(show)?;
                let right = decimal::format_unscaled(want, scale).map_err(show)?;
                Err(format!("decimal {left} != {right}"))
            }
        }
        _ => Err(format!("kind {kind}")),
    }
}

fn check_sum(evidence: &Path, oracle: &Value) -> Result<(), String> {
    let spec = &oracle["sum_check"];
    let table = spec["table"].as_str().ok_or("sum table")?;
    let (batch, _) = read_arrow(&evidence.join(format!("{table}.arrow")))?;
    let column = spec["column"].as_str().ok_or("sum column")?;
    let index = column_index(oracle, table, column)?;
    let values = batch.column(index).as_any().downcast_ref::<Decimal128Array>().ok_or("sum decimal")?;
    let rows = spec["rows"].as_array().ok_or("sum rows")?;
    let into = spec["into"].as_u64().ok_or("sum into")? as usize;
    let mut total: i128 = 0;
    for row in rows {
        total += values.value(row.as_u64().ok_or("row")? as usize);
    }
    if total == values.value(into) {
        Ok(())
    } else {
        Err(format!("{total} != {}", values.value(into)))
    }
}

fn check_order(evidence: &Path, oracle: &Value) -> Result<(), String> {
    let (batch, _) = read_arrow(&evidence.join("str_key.arrow"))?;
    let values = batch.column(0).as_any().downcast_ref::<StringArray>().ok_or("order")?;
    let want = oracle["string_order"].as_array().ok_or("string_order")?;
    if values.len() != want.len() {
        return Err("order length".to_string());
    }
    for (index, expected) in want.iter().enumerate() {
        if values.value(index) != expected.as_str().unwrap_or("") {
            return Err(format!("order {index} {:?} != {:?}", values.value(index), expected));
        }
    }
    Ok(())
}

fn check_nulls(evidence: &Path, oracle: &Value) -> Result<(), String> {
    let (batch, _) = read_arrow(&evidence.join("nz_obs.arrow"))?;
    let values = batch.column(1).as_any().downcast_ref::<Decimal128Array>().ok_or("nz")?;
    let mut nulls = 0;
    let mut zeros = 0;
    for index in 0..values.len() {
        if values.is_null(index) {
            nulls += 1;
        } else if values.value(index) == 0 {
            zeros += 1;
        }
    }
    let want_nulls = oracle["null_vs_zero"]["nulls"].as_u64().ok_or("nulls")?;
    let want_zeros = oracle["null_vs_zero"]["zeros"].as_u64().ok_or("zeros")?;
    if nulls == want_nulls && zeros == want_zeros {
        Ok(())
    } else {
        Err(format!("nulls {nulls} zeros {zeros}"))
    }
}

fn check_caps(evidence: &Path) -> Result<(), String> {
    for name in ["row-cap", "byte-cap"] {
        let body = read_json(&evidence.join(format!("{name}.json")))?;
        if body["ready"] != false || body["arrow"] != false {
            return Err(format!("{name} ready"));
        }
        if evidence.join(format!("{name}.arrow")).exists() {
            return Err(format!("{name} arrow exists"));
        }
    }
    Ok(())
}

fn check_saturation(evidence: &Path) -> Result<(), String> {
    let body = read_json(&evidence.join("saturation.json"))?;
    if body["lossless"] == false {
        Ok(())
    } else {
        Err("saturation marked lossless".to_string())
    }
}

fn column_index(oracle: &Value, table: &str, name: &str) -> Result<usize, String> {
    let columns = oracle["tables"][table]["columns"].as_array().ok_or("columns")?;
    columns
        .iter()
        .position(|column| column["name"] == name)
        .ok_or_else(|| format!("missing {name}"))
}

fn read_arrow(path: &Path) -> Result<(arrow_array::RecordBatch, std::sync::Arc<arrow_schema::Schema>), String> {
    let bytes = fs::read(path).map_err(show)?;
    if bytes.len() < 16 || bytes.starts_with(b"ARROW1") {
        return Err(format!("{} is not an ipc stream", path.display()));
    }
    let continuation = i32::from_le_bytes(bytes[..4].try_into().map_err(show)?);
    let meta_len = i32::from_le_bytes(bytes[4..8].try_into().map_err(show)?);
    if continuation != -1 || meta_len <= 0 || bytes[bytes.len() - 8..] != [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0] {
        return Err(format!("{} missing stream eos", path.display()));
    }
    let reader = StreamReader::try_new(Cursor::new(bytes), None).map_err(show)?;
    let schema = reader.schema();
    let batch = reader
        .into_iter()
        .next()
        .ok_or("empty arrow")?
        .map_err(show)?;
    Ok((batch, schema))
}

fn expect(got: Option<&str>, want: Option<&str>, label: &str) -> Result<(), String> {
    if got == want { Ok(()) } else { Err(format!("{label} {got:?} != {want:?}")) }
}

fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(path).map_err(show)?).map_err(show)
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(show)?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(show)
}
