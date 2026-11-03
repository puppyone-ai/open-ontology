use super::*;
use serde_json::{Value as Json, json};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn table(name: &str) -> TableRef {
    TableRef {
        schema: "fixture".into(),
        name: name.into(),
    }
}
fn config(v: &Json, user: &str) -> MysqlConfig {
    MysqlConfig {
        mysql_host: "127.0.0.1".into(),
        mysql_port: v["port"].as_u64().unwrap() as u16,
        mysql_database: "fixture".into(),
        mysql_user: user.into(),
        mysql_ca_file: v["ca"].as_str().unwrap().into(),
        mysql_tls_server_name: Some("localhost".into()),
        secret_ref: crate::SecretRef::new("secret://lane-c-fixture").unwrap(),
        connect_timeout_ms: 5000,
    }
}
fn password(v: &Json, user: &str) -> Zeroizing<String> {
    Zeroizing::new(fs::read_to_string(v[format!("{user}_password")].as_str().unwrap()).unwrap())
}
fn engine(v: &Json) -> MysqlEngine {
    let mut options = EngineOptions::default();
    options.budget.duty_cycle_percent = 100;
    MysqlEngine::new(config(v, "reader"), password(v, "reader"), options)
}
fn admin(v: &Json, sql: &str) {
    let mut child = Command::new("docker")
        .args([
            "exec",
            "-i",
            v["container"].as_str().unwrap(),
            "mysql",
            "--defaults-extra-file=/fixture/admin.cnf",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(sql.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "fixture admin failed");
}
fn input() -> Json {
    serde_json::from_slice(&fs::read(std::env::var("OO_MYSQL_ENGINE_FIXTURE").unwrap()).unwrap())
        .unwrap()
}
fn save(v: &Json, name: &str, data: Json) {
    fs::write(
        Path::new(v["out"].as_str().unwrap()).join(name),
        serde_json::to_vec_pretty(&data).unwrap(),
    )
    .unwrap();
}

#[test]
fn tagged_wire_and_lineage_are_not_flattened() {
    let id = "3e11fa47-71ca-11e1-9e33-c80aa9429562";
    let mut set = GtidSet::parse(&format!("{id}:1-4:blue:1-3")).unwrap();
    let wire = set.wire().unwrap();
    assert_eq!(wire.len(), 2);
    assert_eq!(wire[1].tag().unwrap().to_string(), "blue");
    set.advance(id, "blue", 4).unwrap();
    assert!(!set.contains_one(id, "", 5));
    assert!(
        set.advance(id, "blue", 6)
            .unwrap_err()
            .fact()
            .unwrap()
            .contains("rebaseline")
    );
}

#[tokio::test]
#[ignore = "owned lane-C binlog fixture"]
async fn live_c1() {
    let v = input();
    let mut e = engine(&v);
    let mut writer = super::super::super::connect(&config(&v, "writer"), &password(&v, "writer"))
        .await
        .unwrap();
    let budget = e.options.budget.clone();
    let empty = ChunkSpec {
        table: table("cdc_empty"),
        plan_revision: 0,
        chunk_id: 0,
        lo: None,
        hi: None,
        estimated_rows: None,
    };
    #[derive(Default)]
    struct Sink(Vec<RecordBatch>);
    impl ChunkSink for Sink {
        fn push(&mut self, batch: RecordBatch) -> Result<()> {
            self.0.push(batch);
            Ok(())
        }
    }
    let mut sink = Sink::default();
    let empty_receipt = e.read_chunk(&empty, &budget, &mut sink).await.unwrap();
    assert_eq!(sink.0.len(), 1);
    assert_eq!(sink.0[0].num_rows(), 0);
    assert_eq!(sink.0[0].num_columns(), 2);
    assert_eq!(empty_receipt.rows, 0);
    let (bytes, hash) = ipc_digest(&sink.0).unwrap();
    assert_eq!((empty_receipt.bytes, empty_receipt.sha256), (bytes, hash));
    let tables = vec![table("cdc_a"), table("cdc_b")];
    let mut changes = e.changes(&tables, 9301).await.unwrap();
    let earliest = changes.earliest_available().await.unwrap();
    assert_eq!(earliest, e.earliest_binlog().await.unwrap());
    let mut cursor = e.watermark().await.unwrap().unwrap();
    let l0 = cursor.clone();
    // A file cursor on the same transaction exercises non-GTID consumption.
    let status = e.basics.as_ref().unwrap().dialect.status_statement;
    let rows = e.exec(status, vec![]).await.unwrap();
    let file_start = Watermark::MysqlFilePos {
        file: row_text(&rows[0], 0).unwrap(),
        position: row_u64(&rows[0], 1).unwrap(),
    };
    let mut file_changes = e.changes(&tables, 9302).await.unwrap();
    let mut target: BTreeMap<(TableRef, Vec<KeyValue>), RecordBatch> = BTreeMap::new();
    let mut counts = Vec::new();
    for cycle in 0..3 {
        if cycle == 0 {
            writer.query_drop("START TRANSACTION").await.unwrap();
            writer
                .query_drop("INSERT INTO fixture.cdc_a VALUES (1,'first'),(2,'second');")
                .await
                .unwrap();
            writer
                .query_drop("INSERT INTO fixture.cdc_b VALUES (1,'other');")
                .await
                .unwrap();
            writer.query_drop("COMMIT").await.unwrap();
        } else if cycle == 1 {
            writer.query_drop("START TRANSACTION").await.unwrap();
            writer
                .query_drop("UPDATE fixture.cdc_a SET id=3,payload='moved' WHERE id=1")
                .await
                .unwrap();
            writer
                .query_drop("DELETE FROM fixture.cdc_a WHERE id=2")
                .await
                .unwrap();
            writer
                .query_drop("UPDATE fixture.cdc_b SET payload='updated' WHERE id=1")
                .await
                .unwrap();
            writer.query_drop("COMMIT").await.unwrap();
        }
        let mut count = 0;
        while let Some(batch) = changes.next_batch(&cursor, &budget).await.unwrap() {
            assert_ne!(cursor, batch.position);
            assert_eq!(batch.changes.len(), batch.images.len());
            if cycle == 0 {
                let by_file = file_changes
                    .next_batch(&file_start, &budget)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(by_file.changes, batch.changes);
                assert_eq!(by_file.images, batch.images);
            }
            for (change, image) in batch.changes.iter().zip(&batch.images) {
                match change {
                    RowChange::Insert { table, key } => {
                        target.insert((table.clone(), key.clone()), image.clone());
                    }
                    RowChange::Update {
                        table,
                        key,
                        old_key,
                    } => {
                        if let Some(old) = old_key {
                            target.remove(&(table.clone(), old.clone()));
                        }
                        target.insert((table.clone(), key.clone()), image.clone());
                    }
                    RowChange::Delete { table, key } => {
                        target.remove(&(table.clone(), key.clone()));
                    }
                }
            }
            count += batch.changes.len();
            cursor = batch.position;
        }
        counts.push(count);
        let mut actual = BTreeMap::new();
        for table in &tables {
            let chunk = ChunkSpec {
                table: table.clone(),
                plan_revision: 0,
                chunk_id: 0,
                lo: None,
                hi: None,
                estimated_rows: None,
            };
            let (_, batches, prints) = e.read_inner(&chunk, &budget).await.unwrap();
            let mut keys = prints.into_iter();
            for batch in batches {
                for i in 0..batch.num_rows() {
                    actual.insert((table.clone(), keys.next().unwrap().key), batch.slice(i, 1));
                }
            }
        }
        assert_eq!(target, actual, "cycle {cycle}");
    }
    assert_eq!(counts, vec![3, 3, 0]);
    // Rollback must not generate a change or move the GTID watermark.
    writer.query_drop("BEGIN").await.unwrap();
    writer
        .query_drop("DELETE FROM fixture.cdc_a")
        .await
        .unwrap();
    writer.query_drop("ROLLBACK").await.unwrap();
    assert!(
        changes
            .next_batch(&cursor, &budget)
            .await
            .unwrap()
            .is_none()
    );
    // ReadBudget rejects a whole large transaction, never a partial commit.
    writer
        .query_drop("INSERT INTO fixture.cdc_a VALUES (4,'a'),(5,'b')")
        .await
        .unwrap();
    let mut tiny = budget.clone();
    tiny.max_chunk_rows = 1;
    assert_eq!(
        changes.next_batch(&cursor, &tiny).await.unwrap_err().kind(),
        SourceErrorKind::Budget
    );
    let batch = changes.next_batch(&cursor, &budget).await.unwrap().unwrap();
    assert_eq!(batch.changes.len(), 2);
    cursor = batch.position;
    let mut tagged = false;
    if !v["version"].as_str().unwrap().starts_with("5.7")
        && !v["version"].as_str().unwrap().starts_with("8.0")
    {
        // Session-only tagging on the fixture writer; product source never sets it.
        writer
            .query_drop("SET SESSION gtid_next='AUTOMATIC:lane_c'")
            .await
            .unwrap();
        writer
            .query_drop("INSERT INTO fixture.cdc_a VALUES (6,'tagged')")
            .await
            .unwrap();
        writer
            .query_drop("SET SESSION gtid_next='AUTOMATIC'")
            .await
            .unwrap();
        let batch = changes.next_batch(&cursor, &budget).await.unwrap().unwrap();
        assert_eq!(batch.changes.len(), 1);
        assert!(
            matches!(&batch.position, Watermark::MysqlGtidSet { gtid_set } if gtid_set.contains("lane_c:1"))
        );
        cursor = batch.position;
        // Tagged set is sent back through the real COM_BINLOG_DUMP_GTID path.
        assert!(
            changes
                .next_batch(&cursor, &budget)
                .await
                .unwrap()
                .is_none()
        );
        tagged = true;
    }
    let bad = Watermark::MysqlGtidSet {
        gtid_set: "11111111-1111-1111-1111-111111111111:1".into(),
    };
    assert!(
        changes
            .next_batch(&bad, &budget)
            .await
            .unwrap_err()
            .fact()
            .unwrap()
            .contains("rebaseline")
    );
    admin(
        &v,
        "FLUSH BINARY LOGS; PURGE BINARY LOGS BEFORE NOW() + INTERVAL 1 DAY;",
    );
    for old in [&l0, &file_start] {
        assert!(
            changes
                .next_batch(old, &budget)
                .await
                .unwrap_err()
                .fact()
                .unwrap()
                .contains("rebaseline")
        );
    }
    save(
        &v,
        "c1.json",
        json!({"cycles": counts, "rollback": true, "budget_retry": true, "file_position": true, "tagged": tagged, "lineage_rebaseline": true, "purge_rebaseline": true, "agent_db_acceptance": "pending_lane_m"}),
    );
    changes.close().await;
    file_changes.close().await;
    e.close().await;
    writer.disconnect().await.unwrap();
}
