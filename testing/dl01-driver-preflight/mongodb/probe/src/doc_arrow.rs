//! Append the seal-scope jsonb text as a non-null utf8 `doc` column.
//! The frozen 19-column batch is opened read-only and is not rewritten.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use arrow_array::Array;
use arrow_array::ArrayRef;
use arrow_array::RecordBatch;
use arrow_array::StringArray;
use arrow_array::cast::AsArray;
use arrow_ipc::reader::FileReader;
use arrow_ipc::writer::FileWriter;
use arrow_ipc::writer::StreamWriter;
use arrow_schema::DataType;
use arrow_schema::Field;
use arrow_schema::FieldRef;
use arrow_schema::Schema;
use serde_json::Value;

const FROZEN_NAMES: [&str; 19] = [
    "_id",
    "code",
    "flag",
    "units",
    "price",
    "created",
    "uid",
    "blob",
    "blob_presence",
    "weight",
    "warehouse",
    "whCode",
    "lane",
    "memo",
    "memo_presence",
    "marks",
    "marks_presence",
    "pack",
    "pack_presence",
];

fn main() {
    if let Err(err) = run() {
        eprintln!("oo-w141-doc-arrow: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let frozen = env_path("FROZEN_ARROW")?;
    let canonical = env_path("CANONICAL_JSON")?;
    let output = env_path("OUT_ARROW")?;
    let identity = env_path("OUT_IDENTITY")?;
    if output.exists() || identity.exists() {
        return Err("output already exists".to_string());
    }
    let docs = load_docs(&canonical)?;
    let batch = read_frozen(&frozen)?;
    if batch.num_rows() != docs.len() {
        return Err(format!("rows {} docs {}", batch.num_rows(), docs.len()));
    }
    let written = append_doc(&batch, &docs)?;
    write_arrow(&output, &written)?;
    let reread = read_one(&output, 20)?;
    if reread.num_rows() != docs.len() || reread.num_columns() != 20 {
        return Err("reread shape".to_string());
    }
    if reread.schema().field(19).name() != "doc" || reread.schema().field(19).is_nullable() {
        return Err("doc field".to_string());
    }
    let column = reread.column(19).as_string::<i32>();
    for (index, expected) in docs.iter().enumerate() {
        if column.is_null(index) || column.value(index) != expected {
            return Err(format!("doc cell {index}"));
        }
    }
    for (index, name) in FROZEN_NAMES.iter().enumerate() {
        if reread.schema().field(index).name() != *name {
            return Err(format!("frozen column {index} moved"));
        }
    }
    let body = serde_json::json!({
        "protocol": "oo.w141.doc-arrow/v1",
        "rows": docs.len(),
        "frozen_columns": FROZEN_NAMES.len(),
        "columns": 20,
        "doc_column": "doc",
        "doc_nullable": false,
        "document_scope": "full-approved",
        "weight_in_document": true,
        "hidden_fields": [],
        "frozen_arrow_replaced": false
    });
    let mut file = File::create(&identity).map_err(|err| err.to_string())?;
    file.write_all(body.to_string().as_bytes()).map_err(|err| err.to_string())?;
    file.write_all(b"\n").map_err(|err| err.to_string())?;
    if let Ok(path) = std::env::var("OUT_STREAM") {
        let file = File::create(path).map_err(|err| err.to_string())?;
        let mut writer = StreamWriter::try_new(file, written.schema().as_ref()).map_err(|err| err.to_string())?;
        writer.write(&written).map_err(|err| err.to_string())?;
        writer.finish().map_err(|err| err.to_string())?;
    }
    println!("rows={} columns=20 doc=utf8", docs.len());
    Ok(())
}

fn env_path(name: &str) -> Result<std::path::PathBuf, String> {
    let value = std::env::var(name).map_err(|_| format!("{name} is unset"))?;
    if value.is_empty() || value.contains('\0') {
        return Err(format!("{name} is empty"));
    }
    Ok(std::path::PathBuf::from(value))
}

fn load_docs(path: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let value: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
    let rows = value.get("rows").and_then(Value::as_array).ok_or("canonical rows")?;
    if rows.is_empty() || rows.len() > 16 {
        return Err(format!("canonical rows {}", rows.len()));
    }
    let mut docs = Vec::with_capacity(16);
    for (index, row) in rows.iter().enumerate() {
        let got = row.get("index").and_then(Value::as_u64).ok_or("canonical index")?;
        if got != u64::try_from(index).unwrap_or(u64::MAX) {
            return Err(format!("canonical index {got}"));
        }
        let doc = row.get("pg_jsonb_text").and_then(Value::as_str).ok_or("pg_jsonb_text")?;
        if doc.as_bytes().contains(&0) {
            return Err(format!("doc {index} contains NUL"));
        }
        let parsed: Value = serde_json::from_str(doc).map_err(|err| format!("doc {index}: {err}"))?;
        if !parsed.is_object() {
            return Err(format!("doc {index} is not an object"));
        }
        reject_hidden(&parsed, &format!("/{index}"))?;
        docs.push(doc.to_string());
    }
    Ok(docs)
}

fn reject_hidden(value: &Value, path: &str) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if matches!(key.as_str(), "hiddenNote" | "notApproved" | "price_odd" | "mixed_marks") {
                    return Err(format!("hidden key {key} at {path}"));
                }
                if key == "weight" {
                    let token = child.get("$numberDouble").and_then(Value::as_str).ok_or("weight is not canonical double")?;
                    let value = token.parse::<f64>().map_err(|_| "invalid double token")?;
                    if !value.is_finite() || value.to_string() != token { return Err("noncanonical double token".to_string()); }
                }
                reject_hidden(child, &format!("{path}.{key}"))?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                reject_hidden(item, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn require_frozen_schema(schema: &Schema) -> Result<(), String> {
    if schema.fields().len() != FROZEN_NAMES.len() {
        return Err(format!("frozen columns {}", schema.fields().len()));
    }
    for (index, name) in FROZEN_NAMES.iter().enumerate() {
        if schema.field(index).name() != *name {
            return Err(format!("column {index} is {}", schema.field(index).name()));
        }
    }
    Ok(())
}

fn read_frozen(path: &Path) -> Result<RecordBatch, String> {
    let batch = read_one(path, FROZEN_NAMES.len())?;
    require_frozen_schema(batch.schema().as_ref())?;
    if batch.num_rows() == 0 || batch.num_rows() > 16 {
        return Err(format!("frozen rows {}", batch.num_rows()));
    }
    Ok(batch)
}

fn read_one(path: &Path, columns: usize) -> Result<RecordBatch, String> {
    let file = File::open(path).map_err(|err| err.to_string())?;
    let reader = FileReader::try_new(file, None).map_err(|err| err.to_string())?;
    if reader.schema().fields().len() != columns {
        return Err(format!("columns {}", reader.schema().fields().len()));
    }
    if reader.num_batches() != 1 {
        return Err(format!("batches {}", reader.num_batches()));
    }
    let mut found = None;
    for item in reader {
        let batch = item.map_err(|err| err.to_string())?;
        if found.is_some() {
            return Err("extra batch".to_string());
        }
        found = Some(batch);
    }
    found.ok_or_else(|| "missing batch".to_string())
}

fn append_doc(batch: &RecordBatch, docs: &[String]) -> Result<RecordBatch, String> {
    let mut fields: Vec<FieldRef> = batch.schema().fields().iter().cloned().collect();
    fields.push(Arc::new(Field::new("doc", DataType::Utf8, false)));
    let schema = Arc::new(Schema::new_with_metadata(fields, batch.schema().metadata().clone()));
    let mut columns: Vec<ArrayRef> = batch.columns().to_vec();
    let doc = StringArray::from_iter_values(docs.iter().map(String::as_str));
    columns.push(Arc::new(doc));
    RecordBatch::try_new(schema, columns).map_err(|err| err.to_string())
}

fn write_arrow(path: &Path, batch: &RecordBatch) -> Result<(), String> {
    let mut file = File::create(path).map_err(|err| err.to_string())?;
    let mut writer = FileWriter::try_new(&mut file, batch.schema().as_ref()).map_err(|err| err.to_string())?;
    writer.write(batch).map_err(|err| err.to_string())?;
    writer.finish().map_err(|err| err.to_string())?;
    file.sync_all().map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::reject_hidden;
    use super::FROZEN_NAMES;

    #[test]
    fn frozen_roster_stays_nineteen_typed_columns() {
        assert_eq!(FROZEN_NAMES.len(), 19);
        assert_eq!(FROZEN_NAMES[0], "_id");
        assert_eq!(FROZEN_NAMES[9], "weight");
        assert_eq!(FROZEN_NAMES[18], "pack_presence");
    }

    #[test]
    fn hidden_document_keys_are_refused() {
        let hidden = serde_json::json!({"code": "S41-000", "weight": 1.5});
        assert!(reject_hidden(&hidden, "/0").is_err());
        let nested = serde_json::json!({"pack": {"notApproved": true}});
        assert!(reject_hidden(&nested, "/0").is_err());
        let clean = serde_json::json!({"code": "S41-000", "flag": 100, "memo": null});
        assert!(reject_hidden(&clean, "/0").is_ok());
        let full = serde_json::json!({"weight": {"$numberDouble": "45.25"}});
        assert!(reject_hidden(&full, "/0").is_ok());
        let unknown = serde_json::json!({"hiddenNote": "private"});
        assert!(reject_hidden(&unknown, "/0").is_err());
    }
}
