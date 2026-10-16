//! Owned-fixture integration. Opt-in configuration contains source references
//! only; it never includes target, Gateway, model or admin credentials.
#![cfg(feature = "mysql")]

use arrow_array::types::{
    Date32Type, DurationMicrosecondType, Int8Type, Int16Type, Int32Type, Int64Type,
    Time64MicrosecondType, TimestampMicrosecondType, UInt64Type,
};
use arrow_array::{
    Array, BinaryArray, BooleanArray, Decimal128Array, Decimal256Array, PrimitiveArray,
    RecordBatch, StringArray,
};
use arrow_ipc::{reader::StreamReader, writer::StreamWriter};
use arrow_schema::DataType;
use mysql_async::prelude::Queryable;
use oo_connectors::{SecretRef, mysql::*, types::RunBinding};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::Path,
};
use zeroize::Zeroizing;

fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().expect(k)
}
fn row_values(batch: &RecordBatch, row: usize, json_columns: &BTreeSet<usize>) -> Vec<Value> {
    batch
        .columns()
        .iter()
        .enumerate()
        .map(|(index, a)| {
            if a.is_null(row) {
                return Value::Null;
            }
            macro_rules! primitive {
                ($t:ty) => {
                    a.as_any()
                        .downcast_ref::<PrimitiveArray<$t>>()
                        .unwrap()
                        .value(row)
                        .to_string()
                };
            }
            let s = match a.data_type() {
                DataType::Int8 => primitive!(Int8Type),
                DataType::Int16 => primitive!(Int16Type),
                DataType::Int32 => primitive!(Int32Type),
                DataType::Int64 => primitive!(Int64Type),
                DataType::UInt64 => primitive!(UInt64Type),
                DataType::Date32 => primitive!(Date32Type),
                DataType::Timestamp(_, _) => primitive!(TimestampMicrosecondType),
                DataType::Duration(_) => primitive!(DurationMicrosecondType),
                DataType::Time64(_) => primitive!(Time64MicrosecondType),
                DataType::Utf8 => a
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .unwrap()
                    .value(row)
                    .to_owned(),
                DataType::Binary => a
                    .as_any()
                    .downcast_ref::<BinaryArray>()
                    .unwrap()
                    .value(row)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
                DataType::Decimal128(_, _) => a
                    .as_any()
                    .downcast_ref::<Decimal128Array>()
                    .unwrap()
                    .value(row)
                    .to_string(),
                DataType::Decimal256(_, _) => a
                    .as_any()
                    .downcast_ref::<Decimal256Array>()
                    .unwrap()
                    .value(row)
                    .to_string(),
                DataType::Boolean => a
                    .as_any()
                    .downcast_ref::<BooleanArray>()
                    .unwrap()
                    .value(row)
                    .to_string(),
                other => panic!("fixture type {other:?}"),
            };
            if json_columns.contains(&index) {
                serde_json::from_str(&s).expect("exact JSON")
            } else {
                json!(s)
            }
        })
        .collect()
}
fn add_rows(
    batch: &RecordBatch,
    rows: &mut BTreeMap<String, Vec<Value>>,
    key: usize,
    json_columns: &BTreeSet<usize>,
) {
    for index in 0..batch.num_rows() {
        let cells = row_values(batch, index, json_columns);
        let k = cells[key].to_string();
        assert!(rows.insert(k, cells).is_none(), "duplicate key");
    }
}
fn write_json(path: &Path, v: &Value) {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    serde_json::to_writer_pretty(&mut f, v).unwrap();
    f.write_all(b"\n").unwrap();
    f.sync_all().unwrap();
}

#[tokio::test]
#[ignore = "requires the two owned TLS MySQL fixtures and private source-only references"]
async fn mysql57_and_mysql84_bounded_arrow_contract() {
    let fixture = std::env::var("OO_MYSQL_MAINLINE_FIXTURES").expect("private fixture path");
    let fixtures: Value = serde_json::from_slice(&fs::read(fixture).unwrap()).unwrap();
    let output = std::env::var("OO_MYSQL_MAINLINE_OUT").expect("new private evidence output");
    let out = Path::new(&output);
    fs::create_dir(out).unwrap();
    fs::set_permissions(out, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let reserved = BTreeSet::from_iter(oo_connectors::PG17_RESERVED.iter().map(|s| s.to_string()));
    let mut results = Vec::new();
    for fixture in fixtures.as_array().unwrap() {
        let config = MysqlConfig {
            mysql_host: text(fixture, "host").to_owned(),
            mysql_port: fixture["port"].as_u64().unwrap() as u16,
            mysql_database: text(fixture, "database").to_owned(),
            mysql_user: text(fixture, "user").to_owned(),
            mysql_ca_file: text(fixture, "ca_file").into(),
            secret_ref: SecretRef::new(text(fixture, "secret_ref")).unwrap(),
            connect_timeout_ms: 5000,
        };
        let password = Zeroizing::new(
            fs::read_to_string(text(fixture, "password_file"))
                .unwrap()
                .trim_end_matches(['\r', '\n'])
                .to_owned(),
        );
        let mut conn = connect(&config, &password).await.unwrap();
        let mut notes = Vec::new();
        let access = inspect_replication(&mut conn, &config.mysql_user, &mut notes)
            .await
            .unwrap();
        assert!(access.version.starts_with(text(fixture, "version_prefix")));
        assert!(!access.ssl_cipher.is_empty());
        let gtid: String = conn
            .query_first("SELECT @@GLOBAL.gtid_executed")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(gtid, text(fixture, "gtid"));
        conn.query_drop("START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY")
            .await
            .unwrap();
        let mut tables = Vec::new();
        for t in fixture["tables"].as_array().unwrap() {
            let selected = SelectedTable {
                schema: config.mysql_database.clone(),
                name: text(t, "name").to_owned(),
                columns: t["columns"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned())
                    .collect(),
            };
            let catalog = catalog_columns(&mut conn, &mut notes, &selected)
                .await
                .unwrap();
            let columns = keep_requested(
                map::map_table(
                    &catalog,
                    false,
                    map::session_utc(&access.time_zone),
                    &reserved,
                )
                .unwrap(),
                &selected.columns,
            )
            .unwrap();
            let key = columns.iter().position(|c| c.key).unwrap();
            let json_columns = columns
                .iter()
                .enumerate()
                .filter_map(|(i, c)| matches!(c.store, map::StoreKind::Json).then_some(i))
                .collect::<BTreeSet<_>>();
            let binding = RunBinding {
                source_id: text(fixture, "source_id").to_owned(),
                schema: selected.schema.clone(),
                table: selected.name.clone(),
                run_generation: fixture["generation"].as_u64().unwrap(),
                input_digest: [0; 32],
            };
            let mut reader = TableReader::new(
                &selected.schema,
                &selected.name,
                columns.clone(),
                binding.clone(),
                1000,
                1000000,
                33554432,
                300000,
                30000,
            )
            .unwrap();
            let mut owned = ReadConnection::new(conn, config.clone(), password.clone());
            let mut actual = BTreeMap::new();
            let mut pages = 0;
            let mut largest = 0;
            let mut writer: Option<StreamWriter<File>> = None;
            let arrow_path = out.join(format!("{}-{}.arrow", text(fixture, "name"), selected.name));
            while let Some(batch) = owned
                .read_chunk(&mut reader, &Cancel::default())
                .await
                .unwrap()
            {
                let arrow = batch.typed.record_batch().unwrap();
                assert!(arrow.num_rows() <= 1000);
                assert!(arrow.get_array_memory_size() <= 16 * 1024 * 1024);
                largest = largest.max(arrow.num_rows());
                pages += 1;
                if writer.is_none() {
                    let file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(&arrow_path)
                        .unwrap();
                    writer = Some(StreamWriter::try_new(file, &arrow.schema()).unwrap());
                }
                writer.as_mut().unwrap().write(&arrow).unwrap();
                add_rows(&arrow, &mut actual, key, &json_columns);
            }
            writer.as_mut().unwrap().finish().unwrap();
            conn = owned.into_connection().unwrap();
            let mut expected = BTreeMap::new();
            for reference in
                StreamReader::try_new(File::open(text(t, "reference_arrow")).unwrap(), None)
                    .unwrap()
            {
                add_rows(&reference.unwrap(), &mut expected, key, &json_columns);
            }
            assert!(actual == expected, "all-cell Arrow/source gold equality");
            let bytes = fs::read(&arrow_path).unwrap();
            tables.push(json!({"table":selected.name,"rows":actual.len(),"columns":columns.len(),"batches":pages,"largest_batch_rows":largest,"different_cells":0,"arrow":arrow_path,"arrow_sha256":format!("{:x}",Sha256::digest(&bytes))}));
        }
        conn.query_drop("ROLLBACK").await.unwrap();
        conn.disconnect().await.unwrap();
        let mut observer = connect(&config, &password).await.unwrap();
        let after: String = observer
            .query_first("SELECT @@GLOBAL.gtid_executed")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after, gtid);
        // Both supplied CA and hostname verification must fail closed.
        for (kind, mut wrong) in [
            ("wrong_ca", config.clone()),
            ("wrong_hostname", config.clone()),
        ] {
            if kind == "wrong_ca" {
                wrong.mysql_ca_file = text(fixture, "wrong_ca_file").into();
            } else {
                wrong.mysql_host = text(fixture, "wrong_hostname").to_owned();
            }
            assert!(connect(&wrong, &password).await.is_err(), "{kind}");
        }
        // Cancellation consumes the exact live connection; it cannot be reused.
        let conn = connect(&config, &password).await.unwrap();
        let id = conn.id();
        let first = &fixture["tables"][0];
        let selected = SelectedTable {
            schema: config.mysql_database.clone(),
            name: text(first, "name").to_owned(),
            columns: vec![],
        };
        let mut probe = connect(&config, &password).await.unwrap();
        let columns = map::map_table(
            &catalog_columns(&mut probe, &mut notes, &selected)
                .await
                .unwrap(),
            false,
            true,
            &reserved,
        )
        .unwrap();
        probe.disconnect().await.unwrap();
        let binding = RunBinding {
            source_id: text(fixture, "source_id").to_owned(),
            schema: selected.schema.clone(),
            table: selected.name.clone(),
            run_generation: fixture["generation"].as_u64().unwrap(),
            input_digest: [0; 32],
        };
        let mut reader = TableReader::new(
            &selected.schema,
            &selected.name,
            columns.clone(),
            binding.clone(),
            1,
            1000000,
            33554432,
            300000,
            30000,
        )
        .unwrap();
        let mut owned = ReadConnection::new(conn, config.clone(), password.clone());
        let cancel = Cancel::default();
        cancel.cancel();
        assert_eq!(
            owned
                .read_chunk(&mut reader, &cancel)
                .await
                .err()
                .unwrap()
                .code,
            "cancel"
        );
        assert!(owned.into_connection().is_err());
        let live: u64 = observer
            .exec_first(
                "SELECT COUNT(*) FROM information_schema.PROCESSLIST WHERE ID=?",
                (id,),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(live, 0);
        // Exact row cap succeeds only at real EOF. A smaller cap refuses excess rows.
        let conn = connect(&config, &password).await.unwrap();
        let id = conn.id();
        let mut owned = ReadConnection::new(conn, config.clone(), password.clone());
        let mut reader = TableReader::new(
            &selected.schema,
            &selected.name,
            columns,
            binding,
            1,
            1,
            33554432,
            300000,
            30000,
        )
        .unwrap();
        owned
            .read_chunk(&mut reader, &Cancel::default())
            .await
            .unwrap();
        assert_eq!(
            owned
                .read_chunk(&mut reader, &Cancel::default())
                .await
                .err()
                .unwrap()
                .fact,
            "max_rows"
        );
        let live: u64 = observer
            .exec_first(
                "SELECT COUNT(*) FROM information_schema.PROCESSLIST WHERE ID=?",
                (id,),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(live, 0);
        observer.disconnect().await.unwrap();
        results.push(json!({"source":fixture["source_id"],"generation":fixture["generation"],"version":access.version,"gtid":gtid,"tables":tables,"cancel_backend_remaining":0,"budget_backend_remaining":0,"wrong_ca_refused":true,"wrong_hostname_refused":true}));
    }
    write_json(
        &out.join("mainline-reader-verification.json"),
        &json!({"protocol":"oo.mysql.mainline-live-reader/v1","sources":results,"source_DML":false,"models":0}),
    );
}
