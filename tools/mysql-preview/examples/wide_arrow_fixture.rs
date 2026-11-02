use oo_connectors::mysql::{map, values};
use oo_connectors::types::{RunBinding, assemble_batch};
use std::collections::BTreeSet;

fn main() {
    let binding = RunBinding {
        source_id: "fixture".into(),
        schema: "sample".into(),
        table: "wide".into(),
        run_generation: 1,
        input_digest: [0; 32],
    };
    let columns = (0..292)
        .map(|i| map::MysqlColumn {
            name: if i == 1 {
                "group".into()
            } else {
                format!("c_{i}")
            },
            ordinal: i + 1,
            data_type: "int".into(),
            column_type: "int".into(),
            nullable: false,
            char_len: None,
            precision: Some(10),
            scale: Some(0),
            charset: None,
            column_key: if i == 0 { "PRI".into() } else { String::new() },
        })
        .map(|c| map::map_capture_column(&c, false, true, &BTreeSet::new()).unwrap())
        .collect::<Vec<_>>();
    let cells = columns
        .iter()
        .map(|c| {
            values::decode_cell(c.store, &values::RawCell::Int(1), false)
                .unwrap()
                .0
        })
        .collect();
    let batch = assemble_batch(
        &binding,
        columns.into_iter().map(|c| c.plan).collect(),
        &[1],
        vec![cells],
    )
    .unwrap()
    .record_batch()
    .unwrap();
    let path = std::env::args().nth(1).unwrap();
    let mut writer = arrow_ipc::writer::StreamWriter::try_new(
        std::fs::File::create(path).unwrap(),
        &batch.schema(),
    )
    .unwrap();
    writer.write(&batch).unwrap();
    writer.finish().unwrap();
}
