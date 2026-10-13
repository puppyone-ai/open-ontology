use std::io::Cursor;

use arrow_ipc::reader::StreamReader;
use arrow_schema::Schema;
use serde_json::{json, Value};

use crate::fail::{fail, Gate};

#[derive(Debug)]
pub struct IpcInfo {
    pub rows: usize,
    pub batches: usize,
    pub fields: Vec<Value>,
}

pub fn inspect_ipc(bytes: &[u8], max_rows: usize, max_bytes: usize) -> Gate<IpcInfo> {
    if bytes.len() > max_bytes {
        return Err(fail(format!(
            "ipc is {} bytes, above the client cap {max_bytes}",
            bytes.len()
        )));
    }
    if bytes.len() < 8 {
        return Err(fail("ipc is truncated: fewer than 8 bytes"));
    }
    let cursor = Cursor::new(bytes);
    let reader = StreamReader::try_new(cursor, None).map_err(|error| {
        fail(format!("ipc schema is not a complete Arrow stream: {error}"))
    })?;
    let schema = reader.schema();
    let fields = schema_fields(schema.as_ref());
    let mut rows = 0usize;
    let mut batches = 0usize;
    for batch in reader {
        let batch = batch.map_err(|error| fail(format!("ipc batch is truncated or invalid: {error}")))?;
        rows = rows.saturating_add(batch.num_rows());
        batches += 1;
        if rows > max_rows {
            return Err(fail(format!("ipc has {rows} rows, above the client cap {max_rows}")));
        }
    }
    Ok(IpcInfo { rows, batches, fields })
}

fn schema_fields(schema: &Schema) -> Vec<Value> {
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{RecordBatch, UInt64Array};
    use arrow_ipc::writer::StreamWriter;
    use arrow_schema::{DataType, Field, Schema};

    use super::inspect_ipc;

    fn stream(rows: Vec<u64>) -> Vec<u8> {
        let schema = Arc::new(Schema::new(vec![Field::new("reading_id", DataType::UInt64, false)]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![Arc::new(UInt64Array::from(rows))],
        )
        .unwrap();
        let mut buf = Vec::new();
        let mut writer = StreamWriter::try_new(&mut buf, &schema).unwrap();
        writer.write(&batch).unwrap();
        writer.finish().unwrap();
        buf
    }

    #[test]
    fn complete_uint64_stream_is_bounded() {
        let bytes = stream(vec![1, u64::MAX]);
        let info = inspect_ipc(&bytes, 10, 1_000_000).unwrap();
        assert_eq!(info.rows, 2);
        assert_eq!(info.batches, 1);
    }

    #[test]
    fn truncated_stream_is_not_ready() {
        let mut bytes = stream(vec![1, 2, 3]);
        let keep = bytes.len().saturating_sub(32).max(4);
        bytes.truncate(keep);
        let error = inspect_ipc(&bytes, 10, 1_000_000).unwrap_err();
        let text = error.0.to_ascii_lowercase();
        assert!(text.contains("truncat") || text.contains("invalid") || text.contains("arrow"), "{text}");
    }

    #[test]
    fn row_cap_rejects_a_complete_stream() {
        let bytes = stream(vec![1, 2, 3]);
        let error = inspect_ipc(&bytes, 1, 1_000_000).unwrap_err();
        assert!(error.0.contains("cap"), "{error}");
    }
}
