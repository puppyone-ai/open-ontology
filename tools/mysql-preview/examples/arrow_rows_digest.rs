use arrow_ipc::reader::StreamReader;
use std::{
    fs,
    io::{BufWriter, Write},
    path::PathBuf,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let input = PathBuf::from(args.next().expect("Arrow stream"));
    let canonical = PathBuf::from(args.next().expect("new canonical output file"));
    let mut output = BufWriter::with_capacity(
        1024 * 1024,
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&canonical)
            .unwrap(),
    );
    let mut count = 0u64;
    let mut batches = 0u64;
    for batch in StreamReader::try_new(fs::File::open(input).unwrap(), None).unwrap() {
        let batch = batch.unwrap();
        batches += 1;
        for row in 0..batch.num_rows() {
            for array in batch.columns() {
                assert_eq!(format!("{:?}", array.data_type()), "Utf8");
                if array.is_null(row) {
                    output.write_all(&[0]).unwrap();
                    continue;
                }
                let data = array.to_data();
                let offset = (data.offset() + row) * 4;
                let indices = data.buffers()[0].as_slice();
                let start =
                    i32::from_le_bytes(indices[offset..offset + 4].try_into().unwrap()) as usize;
                let end = i32::from_le_bytes(indices[offset + 4..offset + 8].try_into().unwrap())
                    as usize;
                let value = &data.buffers()[1].as_slice()[start..end];
                output.write_all(&[1]).unwrap();
                output
                    .write_all(&(value.len() as u64).to_le_bytes())
                    .unwrap();
                output.write_all(value).unwrap();
            }
            count += 1;
        }
    }
    output.flush().unwrap();
    println!(
        "{}",
        serde_json::json!({"rows":count,"batches":batches,"ordered_sha256":oo_connectors::mysql::chunk::file_sha256(&canonical).unwrap()})
    );
}
