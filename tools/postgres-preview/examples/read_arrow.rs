//! Offline full-row oracle helper for the owned bigint/text convergence fixture.
use std::{fs::File, io::{self, Write}};
use arrow_array::{Array, Int64Array, StringArray};
use arrow_ipc::reader::StreamReader;
use serde_json::{Map, Value};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = io::BufWriter::new(io::stdout().lock());
    for path in std::env::args().skip(1) {
        let reader = StreamReader::try_new(File::open(path)?, None)?;
        for batch in reader {
            let batch = batch?;
            for row in 0..batch.num_rows() {
                let mut values = Map::new();
                for (field, array) in batch.schema().fields().iter().zip(batch.columns()) {
                    let value = if array.is_null(row) { Value::Null }
                    else if let Some(a) = array.as_any().downcast_ref::<Int64Array>() {
                        Value::from(a.value(row))
                    } else if let Some(a) = array.as_any().downcast_ref::<StringArray>() {
                        Value::from(a.value(row))
                    } else { return Err("owned oracle supports bigint/text only".into()); };
                    values.insert(field.name().clone(), value);
                }
                serde_json::to_writer(&mut output, &values)?;
                writeln!(output)?;
            }
        }
    }
    output.flush()?;
    Ok(())
}
