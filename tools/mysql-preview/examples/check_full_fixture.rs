use arrow_ipc::reader::StreamReader;
use serde_json::Value;
use std::{env, fs::File};

fn main() {
    let args: Vec<_> = env::args().collect();
    let expected: Vec<Vec<Option<String>>> =
        serde_json::from_reader(File::open(&args[2]).unwrap()).unwrap();
    let reader = StreamReader::try_new(File::open(&args[1]).unwrap(), None).unwrap();
    let mut actual = Vec::new();
    for batch in reader {
        let batch = batch.unwrap();
        assert_eq!(batch.num_columns(), 2);
        for row in 0..batch.num_rows() {
            let mut values = Vec::new();
            for array in batch.columns() {
                assert_eq!(format!("{:?}", array.data_type()), "Utf8");
                if array.is_null(row) {
                    values.push(None);
                    continue;
                }
                let data = array.to_data();
                let index = (data.offset() + row) * 4;
                let offsets = data.buffers()[0].as_slice();
                let start =
                    i32::from_le_bytes(offsets[index..index + 4].try_into().unwrap()) as usize;
                let end =
                    i32::from_le_bytes(offsets[index + 4..index + 8].try_into().unwrap()) as usize;
                values.push(Some(
                    std::str::from_utf8(&data.buffers()[1].as_slice()[start..end])
                        .unwrap()
                        .to_owned(),
                ));
            }
            actual.push(values);
        }
    }
    actual.sort();
    let mut expected = expected;
    expected.sort();
    assert_eq!(actual, expected);
    println!(
        "{}",
        serde_json::json!({"independently_verified_rows":actual.len(),"rows":Value::from(actual)})
    );
}
