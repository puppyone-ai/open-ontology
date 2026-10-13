use std::sync::Arc;

use arrow_array::{ArrayRef, Date32Array, RecordBatch};
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use arrow_ipc::MetadataVersion;
use arrow_schema::{DataType, Field, Schema};
use mariadb_arrow_decode::{decode_bytes, epoch_day_zero};

#[test]
fn epoch_day_zero_is_1970_01_01() {
    assert_eq!(epoch_day_zero(), 0);
}

#[test]
fn date32_stream_decodes_and_a_truncated_stream_fails() {
    let schema = Arc::new(Schema::new(vec![Field::new("d", DataType::Date32, true)]));
    let array: ArrayRef = Arc::new(Date32Array::from(vec![Some(0i32), None]));
    let batch = RecordBatch::try_new(Arc::clone(&schema), vec![array]).unwrap();
    let options = IpcWriteOptions::try_new(8, false, MetadataVersion::V5).unwrap();
    let mut bytes = Vec::new();
    {
        let mut writer = StreamWriter::try_new_with_options(&mut bytes, schema.as_ref(), options).unwrap();
        writer.write(&batch).unwrap();
        writer.finish().unwrap();
    }
    let decoded = decode_bytes(&bytes).unwrap();
    assert_eq!(decoded["rows"][0][0], "1970-01-01");
    assert!(decoded["rows"][1][0].is_null());
    let chopped = &bytes[..bytes.len() / 2];
    assert!(decode_bytes(chopped).is_err());
}
