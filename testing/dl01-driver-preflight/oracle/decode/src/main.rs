use arrow_array::Array;
use arrow_array::cast::AsArray;
use arrow_ipc::reader::FileReader;
use arrow_schema::DataType;
use arrow_schema::TimeUnit;
use serde_json::Value;
use serde_json::json;
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("oracle-arrow-decode: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut arrow_path = None;
    let mut gold_path = None;
    let mut out_path = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--arrow" => arrow_path = args.next(),
            "--gold" => gold_path = args.next(),
            "--out" => out_path = args.next(),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let arrow_path = arrow_path.ok_or("missing --arrow")?;
    let gold_path = gold_path.ok_or("missing --gold")?;
    let out_path = out_path.ok_or("missing --out")?;
    let gold: Value = serde_json::from_str(
        &fs::read_to_string(&gold_path).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let file = fs::File::open(&arrow_path).map_err(|e| e.to_string())?;
    let reader = FileReader::try_new(file, None).map_err(|e| e.to_string())?;
    let schema = reader.schema();
    let batches: Vec<_> = reader
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if batches.len() != 1 {
        return fail(&out_path, format!("batch count {}", batches.len()));
    }
    let batch = &batches[0];
    let rows = gold["rows"].as_array().ok_or("gold rows")?;
    let columns = gold["columns"].as_array().ok_or("gold columns")?;
    if batch.num_rows() != rows.len() {
        return fail(
            &out_path,
            format!("row count {} != {}", batch.num_rows(), rows.len()),
        );
    }
    if schema.fields().len() != columns.len() {
        return fail(&out_path, "column count mismatch".into());
    }
    for (index, column) in columns.iter().enumerate() {
        let name = column["name"].as_str().unwrap();
        let field = schema.field(index);
        if field.name() != name {
            return fail(&out_path, format!("field {index} name {}", field.name()));
        }
        let actual = arrow_type(field.data_type());
        let expect = column["arrow"].as_str().unwrap();
        if actual != expect {
            return fail(
                &out_path,
                format!("field {name} type {actual} != {expect}"),
            );
        }
        if name == "N1" && matches!(field.data_type(), DataType::Boolean) {
            return fail(&out_path, "NUMBER(1) was decoded as Boolean".into());
        }
    }
    for (row_index, gold_row) in rows.iter().enumerate() {
        check_row(batch, row_index, gold_row)?;
    }
    let body = json!({
        "ready": true,
        "rows": batch.num_rows(),
        "schema": schema.fields().iter().map(|f| format!("{}:{}", f.name(), arrow_type(f.data_type()))).collect::<Vec<_>>(),
        "decoder": "oracle-arrow-decode",
        "driver": "none"
    });
    fs::write(&out_path, serde_json::to_string_pretty(&body).unwrap() + "\n")
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn fail(out_path: &str, message: String) -> Result<(), String> {
    let body = json!({"ready": false, "error": message});
    let _ = fs::write(out_path, serde_json::to_string_pretty(&body).unwrap() + "\n");
    Err(message)
}

fn check_row(
    batch: &arrow_array::RecordBatch,
    row_index: usize,
    gold: &Value,
) -> Result<(), String> {
    let id = batch.column(0).as_primitive::<arrow_array::types::Int16Type>().value(row_index);
    if i64::from(id) != gold["ID"].as_i64().unwrap() {
        return Err(format!("row {row_index} id"));
    }
    check_decimal(batch, 1, row_index, &gold["N16"], 16, 6)?;
    check_i8(batch, 2, row_index, &gold["N1"])?;
    check_date(batch, 3, row_index, gold["D"].as_str().unwrap())?;
    check_utf8(batch, 4, row_index, &gold["C6"])?;
    check_utf8(batch, 5, row_index, &gold["V2"])?;
    check_raw(batch, 6, row_index, &gold["R"])?;
    Ok(())
}

fn check_decimal(
    batch: &arrow_array::RecordBatch,
    col: usize,
    row: usize,
    gold: &Value,
    precision: u8,
    scale: i8,
) -> Result<(), String> {
    let array = batch.column(col).as_primitive::<arrow_array::types::Decimal128Type>();
    if array.precision() != precision || array.scale() != scale {
        return Err(format!("decimal precision {} scale {}", array.precision(), array.scale()));
    }
    if gold.is_null() {
        if !array.is_null(row) {
            return Err(format!("row {row} decimal not null"));
        }
        return Ok(());
    }
    if array.is_null(row) {
        return Err(format!("row {row} decimal null"));
    }
    let expect = unscaled(gold.as_str().unwrap(), scale)?;
    if array.value(row) != expect {
        return Err(format!("row {row} decimal {} != {expect}", array.value(row)));
    }
    Ok(())
}

fn check_i8(
    batch: &arrow_array::RecordBatch,
    col: usize,
    row: usize,
    gold: &Value,
) -> Result<(), String> {
    let array = batch.column(col).as_primitive::<arrow_array::types::Int8Type>();
    if gold.is_null() {
        if !array.is_null(row) {
            return Err(format!("row {row} n1 not null"));
        }
        return Ok(());
    }
    let expect = i8::try_from(gold.as_i64().unwrap()).map_err(|_| "n1 range")?;
    if array.is_null(row) || array.value(row) != expect {
        return Err(format!("row {row} n1"));
    }
    Ok(())
}

fn check_utf8(
    batch: &arrow_array::RecordBatch,
    col: usize,
    row: usize,
    gold: &Value,
) -> Result<(), String> {
    let array = batch.column(col).as_string::<i32>();
    if gold.is_null() {
        if !array.is_null(row) {
            return Err(format!("row {row} utf8 not null"));
        }
        return Ok(());
    }
    let expect = gold.as_str().unwrap();
    if array.is_null(row) || array.value(row) != expect {
        return Err(format!(
            "row {row} utf8 {:?} != {:?}",
            array.is_null(row).then_some(None).unwrap_or_else(|| Some(array.value(row))),
            expect
        ));
    }
    Ok(())
}

fn check_raw(
    batch: &arrow_array::RecordBatch,
    col: usize,
    row: usize,
    gold: &Value,
) -> Result<(), String> {
    let array = batch.column(col).as_binary::<i32>();
    if gold.is_null() {
        if !array.is_null(row) {
            return Err(format!("row {row} raw not null"));
        }
        return Ok(());
    }
    let expect = hex_decode(gold.as_str().unwrap())?;
    if array.is_null(row) || array.value(row) != expect {
        return Err(format!("row {row} raw"));
    }
    Ok(())
}

fn check_date(
    batch: &arrow_array::RecordBatch,
    col: usize,
    row: usize,
    gold: &str,
) -> Result<(), String> {
    let array = batch
        .column(col)
        .as_primitive::<arrow_array::types::TimestampMicrosecondType>();
    let expect = civil_micros(gold)?;
    if array.is_null(row) || array.value(row) != expect {
        return Err(format!("row {row} date {} != {expect}", array.value(row)));
    }
    Ok(())
}

fn unscaled(text: &str, scale: i8) -> Result<i128, String> {
    let neg = text.starts_with('-');
    let text = text.trim_start_matches('-');
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    if frac.len() > scale as usize {
        return Err(format!("gold scale {}", frac.len()));
    }
    let mut digits = format!("{whole}{frac}");
    digits.push_str(&"0".repeat(scale as usize - frac.len()));
    let mut value: i128 = digits.parse().map_err(|_| format!("decimal {text}"))?;
    if neg {
        value = -value;
    }
    Ok(value)
}

fn hex_decode(text: &str) -> Result<Vec<u8>, String> {
    if text.len() % 2 != 0 {
        return Err("odd hex".into());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| "hex".into()))
        .collect()
}

fn civil_micros(text: &str) -> Result<i64, String> {
    let (date, time) = text.split_once('T').ok_or("date")?;
    let mut date = date.split('-');
    let year: i32 = date.next().ok_or("y")?.parse().map_err(|_| "y")?;
    let month: u32 = date.next().ok_or("m")?.parse().map_err(|_| "m")?;
    let day: u32 = date.next().ok_or("d")?.parse().map_err(|_| "d")?;
    let mut time = time.split(':');
    let hour: u32 = time.next().ok_or("h")?.parse().map_err(|_| "h")?;
    let minute: u32 = time.next().ok_or("min")?.parse().map_err(|_| "min")?;
    let second: u32 = time.next().ok_or("s")?.parse().map_err(|_| "s")?;
    let days = days_from_civil(year, month, day);
    Ok(days * 86_400 * 1_000_000
        + i64::from(hour) * 3_600 * 1_000_000
        + i64::from(minute) * 60 * 1_000_000
        + i64::from(second) * 1_000_000)
}

fn days_from_civil(mut year: i32, month: u32, day: u32) -> i64 {
    year -= i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = (year - era * 400) as u64;
    let month_prime = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * u64::from(month_prime) + 2) / 5 + u64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    i64::from(era) * 146097 + doe as i64 - 719468
}

fn arrow_type(data_type: &DataType) -> String {
    match data_type {
        DataType::Int8 => "Int8".into(),
        DataType::Int16 => "Int16".into(),
        DataType::Boolean => "Boolean".into(),
        DataType::Utf8 => "Utf8".into(),
        DataType::Binary => "Binary".into(),
        DataType::Float64 => "Float64".into(),
        DataType::Decimal128(precision, scale) => format!("Decimal128({precision},{scale})"),
        DataType::Timestamp(TimeUnit::Microsecond, None) => "Timestamp(Microsecond,None)".into(),
        DataType::Timestamp(unit, tz) => format!("Timestamp({unit:?},{tz:?})"),
        other => format!("{other:?}"),
    }
}
