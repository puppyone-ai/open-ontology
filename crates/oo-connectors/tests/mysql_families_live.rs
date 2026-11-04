//! Disposable fixture only, created by testing/engines/mariadb/run.py.
#![cfg(feature = "mysql")]
use arrow_array::RecordBatch;
use mysql_async::prelude::Queryable;
use oo_connectors::{
    SecretRef, SourceError,
    engine::*,
    mysql::{
        self, EngineOptions, MysqlConfig,
        c5::{Cycle, Hints},
        mariadb::MariadbEngine,
        oceanbase::OceanbaseEngine,
        tidb::TidbEngine,
    },
};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};
use zeroize::Zeroizing;

#[derive(Default)]
struct Sink(Vec<RecordBatch>);
impl ChunkSink for Sink {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        self.0.push(batch);
        Ok(())
    }
}

fn config(v: &Value, user: &str) -> MysqlConfig {
    MysqlConfig {
        mysql_host: "127.0.0.1".into(),
        mysql_port: v["port"].as_u64().unwrap() as u16,
        mysql_database: if user == "root" {
            "information_schema"
        } else {
            "fixture"
        }
        .into(),
        mysql_user: if v["family"] == "oceanbase" {
            format!("{user}@test")
        } else {
            user.into()
        },
        mysql_ca_file: v["ca"].as_str().unwrap().into(),
        mysql_tls_server_name: Some("localhost".into()),
        secret_ref: SecretRef::new("secret://lane-c-family-fixture").unwrap(),
        connect_timeout_ms: 3000,
    }
}
fn password(v: &Value, user: &str) -> Zeroizing<String> {
    Zeroizing::new(fs::read_to_string(v[format!("{user}_password")].as_str().unwrap()).unwrap())
}
fn save(v: &Value, name: &str, body: Value) {
    fs::write(
        Path::new(v["out"].as_str().unwrap()).join(name),
        serde_json::to_vec_pretty(&body).unwrap(),
    )
    .unwrap();
}

async fn exercise(engine: &mut impl SourceEngine, admin: &mut mysql_async::Conn, v: &Value) {
    let report = engine.probe().await.unwrap();
    assert_ne!(report.family, EngineFamily::Mysql);
    assert_eq!(
        report.capabilities[&Capability::ChunkedSnapshot],
        CapabilityState::Available
    );
    if report.family != EngineFamily::Mariadb {
        assert!(matches!(
            report.capabilities[&Capability::LogCdc],
            CapabilityState::Unsupported { .. }
        ));
    }
    save(v, "probe.json", json!(report));
    let budget = ReadBudget {
        max_chunk_rows: 2,
        duty_cycle_percent: 100,
        ..Default::default()
    };
    let mut receipts = vec![];
    for (name, count) in [
        ("rows", 6),
        ("text_rows", 3),
        ("composite_rows", 4),
        ("no_key", 3),
        ("empty_rows", 0),
        ("typed_rows", 2),
    ] {
        let table = TableRef {
            schema: "fixture".into(),
            name: name.into(),
        };
        let read_budget = if name == "no_key" {
            ReadBudget {
                max_chunk_rows: 10,
                ..budget.clone()
            }
        } else {
            budget.clone()
        };
        let plan = engine.plan_chunks(&table, &read_budget).await.unwrap();
        save(v, &format!("plan-{name}.json"), json!(plan));
        let expected = whole_table(admin, name).await;
        let mut rows = 0;
        let mut chunks = vec![];
        for chunk in &plan.chunks {
            let mut sink = Sink::default();
            let receipt = engine
                .read_chunk(chunk, &read_budget, &mut sink)
                .await
                .unwrap();
            assert!(!sink.0.is_empty(), "schema-bearing empty batch");
            assert_eq!(
                receipt.rows,
                sink.0.iter().map(|b| b.num_rows() as u64).sum::<u64>()
            );
            for batch in &sink.0 {
                for (column, actual) in batch.columns().iter().enumerate() {
                    assert_eq!(
                        actual,
                        &expected
                            .column(column)
                            .slice(rows as usize, batch.num_rows()),
                        "independent whole-table oracle {name}"
                    );
                }
                rows += batch.num_rows() as u64;
            }
            chunks.push(receipt);
        }
        assert_eq!(rows, count, "{name}");
        receipts.push(json!({"table":name,"plan":plan,"receipts":chunks}));
    }
    assert_eq!(engine.discover_tables(&budget).await.unwrap().len(), 5);
    save(v, "snapshot.json", json!(receipts));
    let table = TableRef {
        schema: "fixture".into(),
        name: "rows".into(),
    };
    let plan = engine.plan_chunks(&table, &budget).await.unwrap();
    let mut cycle = Cycle::new(plan.chunks, 10).unwrap();
    // Keep the original ranges; permit growth inside the unbounded final range.
    let budget = ReadBudget {
        max_chunk_rows: 10,
        ..budget
    };
    let initial = cycle
        .prepare(engine, &budget, &Hints::default())
        .await
        .unwrap();
    assert_eq!(initial.replacements.iter().map(|r| r.rows).sum::<u64>(), 6);
    cycle.confirm(initial).unwrap();
    let unchanged = cycle
        .prepare(engine, &budget, &Hints::default())
        .await
        .unwrap();
    assert!(unchanged.replacements.is_empty());
    cycle.confirm(unchanged).unwrap();
    admin
        .query_drop("UPDATE fixture.`rows` SET payload='updated' WHERE id=2")
        .await
        .unwrap();
    admin
        .query_drop("DELETE FROM fixture.`rows` WHERE id=3")
        .await
        .unwrap();
    admin
        .query_drop("INSERT INTO fixture.`rows` VALUES (9,'inserted')")
        .await
        .unwrap();
    admin
        .query_drop("UPDATE fixture.`rows` SET id=10 WHERE id=6")
        .await
        .unwrap();
    let changed = cycle
        .prepare(engine, &budget, &Hints::default())
        .await
        .unwrap();
    assert!(!changed.replacements.is_empty());
    // Compare each replacement against a fresh read, not just aggregate counts.
    for replacement in &changed.replacements {
        let mut sink = Sink::default();
        engine
            .read_chunk(&replacement.chunk, &budget, &mut sink)
            .await
            .unwrap();
        assert_eq!(replacement.batches, sink.0);
    }
    cycle.confirm(changed).unwrap();
    let unchanged = cycle
        .prepare(engine, &budget, &Hints::default())
        .await
        .unwrap();
    assert!(unchanged.replacements.is_empty());
    cycle.confirm(unchanged).unwrap();
    save(
        v,
        "c5.json",
        json!({"cycles":4,"mutation_types":["insert","update","delete","primary_key_update"],"no_change_replacements":0,"replacement_comparison":"Arrow exact equality"}),
    );
}

#[tokio::test]
#[ignore = "owned disposable database and TLS fixture required"]
async fn family_snapshot_c5() {
    let v: Value = serde_json::from_slice(
        &fs::read(std::env::var("OO_FAMILY_FIXTURE").expect("fixture config")).unwrap(),
    )
    .unwrap();
    let root_password = if v["family"] == "tidb" {
        Zeroizing::new(String::new())
    } else {
        password(&v, "root")
    };
    let mut admin = None;
    for _ in 0..120 {
        if let Ok(conn) = mysql::connect(&config(&v, "root"), &root_password).await {
            admin = Some(conn);
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let mut admin = admin.expect("fixture TLS root connection");
    let max: Vec<mysql_async::Row> = admin
        .query("SELECT @@GLOBAL.max_connections")
        .await
        .unwrap();
    let load: Vec<mysql_async::Row> = admin
        .query("SHOW GLOBAL STATUS WHERE Variable_name IN ('Threads_running', 'Threads_connected')")
        .await
        .unwrap();
    save(
        &v,
        "load-diagnostics.json",
        json!({"max":format!("{max:?}"),"load":format!("{load:?}")}),
    );
    for sql in [
        "CREATE DATABASE fixture CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci",
        "CREATE TABLE fixture.`rows` (id INT PRIMARY KEY, payload VARCHAR(64))",
        "INSERT INTO fixture.`rows` VALUES (1,NULL),(2,''),(3,'three'),(4,'four'),(5,'five'),(6,'six')",
        "CREATE TABLE fixture.text_rows (k VARCHAR(40) COLLATE utf8mb4_general_ci PRIMARY KEY, payload VARCHAR(64))",
        "INSERT INTO fixture.text_rows VALUES ('a','lower'),('B','upper'),('trail ','space')",
        "CREATE TABLE fixture.composite_rows (tenant VARCHAR(40), seq INT, payload VARCHAR(64), PRIMARY KEY(tenant,seq))",
        "INSERT INTO fixture.composite_rows VALUES ('a',1,'one'),('a',2,NULL),('b',1,''),('b',2,'two')",
        "CREATE TABLE fixture.no_key (id INT, payload VARCHAR(64))",
        "INSERT INTO fixture.no_key VALUES (1,NULL),(1,NULL),(2,'')",
        "CREATE TABLE fixture.empty_rows LIKE fixture.`rows`",
        "CREATE TABLE fixture.typed_rows (id INT PRIMARY KEY, amount DECIMAL(30,8), big BIGINT UNSIGNED, doc JSON, raw VARBINARY(20), elapsed TIME(6))",
        "INSERT INTO fixture.typed_rows VALUES (1,12345678901234567890.12345678,18446744073709551615,'{\"a\":1}',0x00ff,'-27:02:03.123456'),(2,NULL,0,'null',X'','00:00:00')",
    ] {
        admin.query_drop(sql).await.unwrap();
    }
    let storage: Vec<mysql_async::Row> = admin.query("SELECT TABLE_NAME, ENGINE FROM information_schema.TABLES WHERE TABLE_SCHEMA = 'fixture'").await.unwrap();
    save(
        &v,
        "storage.json",
        json!(
            storage
                .iter()
                .map(|r| (r.get::<String, _>(0), r.get::<String, _>(1)))
                .collect::<Vec<_>>()
        ),
    );
    let secret = password(&v, "reader");
    admin
        .query_drop(
            Zeroizing::new(format!(
                "CREATE USER 'reader'@'%' IDENTIFIED BY '{}'",
                &*secret
            ))
            .as_str(),
        )
        .await
        .expect("create reader");
    macro_rules! denied_probe {
        ($engine:ident) => {{
            let mut denied = $engine::new(config(&v, "reader"), secret.clone(), Default::default());
            let report = denied.probe().await.unwrap();
            assert!(matches!(
                report.capabilities[&Capability::Catalog],
                CapabilityState::NeedsSetup { .. }
            ));
            assert!(
                report
                    .requirements()
                    .iter()
                    .any(|r| r.name == "SELECT" && r.dba_statement.is_some())
            );
            save(&v, "probe-no-select.json", json!(report));
            denied.close().await;
        }};
    }
    match v["family"].as_str().unwrap() {
        "mariadb" => denied_probe!(MariadbEngine),
        "tidb" => denied_probe!(TidbEngine),
        "oceanbase" => denied_probe!(OceanbaseEngine),
        _ => unreachable!(),
    }
    admin
        .query_drop("GRANT SELECT ON fixture.* TO 'reader'@'%'")
        .await
        .unwrap();
    if v["family"] == "tidb" || v["family"] == "oceanbase" {
        admin
            .query_drop("GRANT PROCESS ON *.* TO 'reader'@'%'")
            .await
            .unwrap();
    }
    let options = EngineOptions {
        protection: mysql::protect::Protection {
            max_pause: Duration::from_secs(2),
            ..Default::default()
        },
        budget: ReadBudget {
            duty_cycle_percent: 100,
            ..Default::default()
        },
        ..Default::default()
    };
    match v["family"].as_str().unwrap() {
        "mariadb" => {
            let mut e = MariadbEngine::new(config(&v, "reader"), secret, options);
            exercise(&mut e, &mut admin, &v).await;
            #[cfg(feature = "mysql-binlog")]
            mariadb_wire_probe(&mut admin, &v).await;
            #[cfg(feature = "mysql-binlog")]
            mariadb_c1(&mut e, &mut admin, &v).await;
            e.close().await;
        }
        "tidb" => {
            let mut e = TidbEngine::new(config(&v, "reader"), secret, options);
            exercise(&mut e, &mut admin, &v).await;
            e.close().await;
        }
        "oceanbase" => {
            let mut e = OceanbaseEngine::new(config(&v, "reader"), secret, options);
            exercise(&mut e, &mut admin, &v).await;
            e.close().await;
        }
        _ => panic!("unknown fixture family"),
    }
    admin.disconnect().await.unwrap();
}

#[cfg(feature = "mysql-binlog")]
async fn mariadb_wire_probe(admin: &mut mysql_async::Conn, v: &Value) {
    use futures_util::StreamExt;
    use mysql_async::{BinlogStreamRequest, binlog::events::EventData};
    admin
        .query_drop("GRANT REPLICATION SLAVE, BINLOG MONITOR ON *.* TO 'reader'@'%'")
        .await
        .unwrap();
    let row: mysql_async::Row = admin
        .query_first("SHOW BINLOG STATUS")
        .await
        .unwrap()
        .unwrap();
    let file: String = row.get(0).unwrap();
    let position: u64 = row.get(1).unwrap();
    admin
        .query_drop("INSERT INTO fixture.`rows` VALUES (11,'binlog-probe')")
        .await
        .unwrap();
    let conn = mysql::connect(&config(v, "reader"), &password(v, "reader"))
        .await
        .unwrap();
    let mut stream = conn
        .get_binlog_stream(
            BinlogStreamRequest::new(732)
                .with_non_blocking()
                .with_filename(file.as_bytes())
                .with_pos(position),
        )
        .await
        .unwrap();
    let mut events = vec![];
    let mut outcome = "no_commit";
    for _ in 0..32 {
        let next = tokio::time::timeout(Duration::from_secs(5), stream.next()).await;
        let Ok(Some(Ok(event))) = next else {
            break;
        };
        let data = event.read_data();
        match data {
            Ok(Some(EventData::XidEvent(_))) => {
                events.push("xid");
                outcome = "decodable_commit";
                break;
            }
            Ok(Some(EventData::RotateEvent(_))) => events.push("rotate"),
            Ok(Some(EventData::FormatDescriptionEvent(_))) => events.push("format"),
            Ok(Some(EventData::QueryEvent(q))) => {
                save(
                    v,
                    &format!("wire-query-{}.json", events.len()),
                    json!({"query":q.query()}),
                );
                events.push("query");
            }
            Ok(Some(EventData::RowsEvent(_))) => events.push("rows"),
            Ok(Some(EventData::TableMapEvent(_))) => events.push("table_map"),
            Ok(None) => {
                events.push("unknown_event");
                outcome = "unsupported_driver_event";
                break;
            }
            _ => {
                events.push("other_event");
                outcome = "unvalidated_event";
                break;
            }
        }
    }
    let _ = stream.close().await;
    save(
        v,
        "mariadb-binlog-wire.json",
        json!({"outcome":outcome,"events":events,"scope":"driver framing probe, not C1 acceptance"}),
    );
}

#[cfg(feature = "mysql-binlog")]
async fn mariadb_c1(engine: &mut MariadbEngine, admin: &mut mysql_async::Conn, v: &Value) {
    admin
        .query_drop(
            "GRANT REPLICATION SLAVE, BINLOG MONITOR, REPLICA MONITOR ON *.* TO 'reader'@'%'",
        )
        .await
        .unwrap();
    engine.close().await;
    let report = engine.probe().await.unwrap();
    assert_eq!(
        report.capabilities[&Capability::LogCdc],
        CapabilityState::Available
    );
    save(v, "c1-probe.json", json!(report));
    let table = TableRef {
        schema: "fixture".into(),
        name: "rows".into(),
    };
    let budget = ReadBudget {
        max_chunk_rows: 100,
        duty_cycle_percent: 100,
        ..Default::default()
    };
    let mut changes = engine.changes(&[table.clone()], 733).await.unwrap();
    let retained = changes.retention(0, 0).await.unwrap();
    assert!(retained.oldest_event_unix.is_some());
    let mut cursor = engine.watermark().await.unwrap().unwrap();
    assert!(matches!(cursor, Watermark::MysqlFilePos { .. }));
    assert!(
        changes
            .next_batch(&cursor, &budget)
            .await
            .unwrap()
            .is_none()
    );
    let mut counts = vec![];
    let mut previous_image = None;
    for (step, sql) in [
        "INSERT INTO fixture.`rows` VALUES (12,'c1')",
        "UPDATE fixture.`rows` SET id=13,payload='changed' WHERE id=12",
        "DELETE FROM fixture.`rows` WHERE id=13",
    ]
    .into_iter()
    .enumerate()
    {
        admin.query_drop(sql).await.unwrap();
        let batch = changes.next_batch(&cursor, &budget).await.unwrap().unwrap();
        assert_eq!(batch.changes.len(), 1);
        assert_eq!(batch.images.iter().map(|b| b.num_rows()).sum::<usize>(), 1);
        match (&batch.changes[0], step) {
            (RowChange::Insert { key, .. }, 0) => assert_eq!(key, &vec![KeyValue::Int(12)]),
            (RowChange::Update { key, old_key, .. }, 1) => {
                assert_eq!(key, &vec![KeyValue::Int(13)]);
                assert_eq!(old_key.as_ref().unwrap(), &vec![KeyValue::Int(12)]);
            }
            (RowChange::Delete { key, .. }, 2) => assert_eq!(key, &vec![KeyValue::Int(13)]),
            other => panic!("unexpected change: {other:?}"),
        }
        if step < 2 {
            let id = if step == 0 { 12 } else { 13 };
            let chunk = ChunkSpec {
                table: table.clone(),
                plan_revision: 0,
                chunk_id: 0,
                lo: Some(vec![KeyValue::Int(id)]),
                hi: Some(vec![KeyValue::Int(id + 1)]),
                estimated_rows: Some(1),
            };
            let mut sink = Sink::default();
            engine.read_chunk(&chunk, &budget, &mut sink).await.unwrap();
            assert_eq!(
                batch.images, sink.0,
                "C1 post-image must equal snapshot Arrow"
            );
            previous_image = Some(batch.images.clone());
        } else {
            assert_eq!(
                batch.images,
                previous_image.as_ref().unwrap().clone(),
                "delete before-image"
            );
        }
        assert_ne!(batch.position, cursor);
        counts.push(batch.changes.len());
        cursor = batch.position;
        assert!(
            changes
                .next_batch(&cursor, &budget)
                .await
                .unwrap()
                .is_none()
        );
    }
    save(
        v,
        "c1.json",
        json!({"cycles":3,"changes":counts,"no_change_batches":0,"position":cursor,"retained_timestamp":retained.oldest_event_unix}),
    );
    admin
        .query_drop("ALTER TABLE fixture.`rows` ADD COLUMN added INT")
        .await
        .unwrap();
    assert_eq!(
        changes
            .next_batch(&cursor, &budget)
            .await
            .unwrap_err()
            .fact(),
        Some("binlog_statement_new_generation_rebaseline")
    );
    changes.close().await;
}

// Full SELECT is independent of range construction, page SQL and C5 digests.
async fn whole_table(admin: &mut mysql_async::Conn, name: &str) -> RecordBatch {
    use mysql::{
        SelectedTable, catalog_columns, map::map_capture_column, raw_cell, values::decode_mapped,
    };
    use oo_connectors::types::{RunBinding, assemble_batch};
    let selected = SelectedTable {
        schema: "fixture".into(),
        name: name.into(),
        columns: vec![],
    };
    let catalog = catalog_columns(admin, &mut vec![], &selected)
        .await
        .unwrap();
    let columns = catalog
        .iter()
        .map(|c| map_capture_column(c, false, true, &Default::default()).unwrap())
        .collect::<Vec<_>>();
    let order = match name {
        "text_rows" => "k",
        "composite_rows" => "tenant,seq",
        _ => "id",
    };
    let rows: Vec<mysql_async::Row> = admin
        .query(format!("SELECT * FROM fixture.`{name}` ORDER BY {order}"))
        .await
        .unwrap();
    let decoded = rows
        .iter()
        .map(|r| {
            columns
                .iter()
                .enumerate()
                .map(|(i, c)| decode_mapped(c, &raw_cell(r, i).unwrap()).unwrap().0)
                .collect()
        })
        .collect();
    assemble_batch(
        &RunBinding {
            source_id: "oracle".into(),
            schema: "fixture".into(),
            table: name.into(),
            run_generation: 0,
            input_digest: [0; 32],
        },
        columns.iter().map(|c| c.plan.clone()).collect(),
        &columns
            .iter()
            .filter(|c| c.key)
            .map(|c| c.plan.attnum)
            .collect::<Vec<_>>(),
        decoded,
    )
    .unwrap()
    .record_batch()
    .unwrap()
}
