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
    let tables = vec![table("cdc_a"), table("cdc_b"), table("cdc_typed")];
    let mut changes = e.changes(&tables, 9301).await.unwrap();
    let earliest = changes.earliest_available().await.unwrap();
    assert_eq!(earliest, e.earliest_binlog().await.unwrap());
    let retention = changes
        .retention(0, 0)
        .await
        .unwrap()
        .require_sufficient()
        .unwrap();
    assert_eq!(retention.earliest, earliest);
    assert!(retention.oldest_event_unix.is_some());
    let short = changes
        .retention(86_400, 86_400)
        .await
        .unwrap()
        .require_sufficient()
        .unwrap_err();
    assert_eq!(short.kind, RequirementKind::Retention);
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
            writer.query_drop("INSERT INTO fixture.cdc_typed VALUES (18446744073709551615,'tenant',12345678901234567890.12345678,JSON_OBJECT('a',1,'b',true),'2026-11-04 00:00:00.123456','-25:01:02.123456','2026-11-04 00:00:00.123456',0x00ff,0)")
                .await.unwrap();
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
            writer.query_drop("UPDATE fixture.cdc_typed SET shard='moved',amount=-0.12345678,doc=JSON_OBJECT('a',2),binary_value=NULL,yr=2026")
                .await.unwrap();
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
    assert_eq!(counts, vec![4, 4, 0]);
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
    let reset = if v["version"].as_str().unwrap().starts_with("5.7")
        || v["version"].as_str().unwrap().starts_with("8.0")
    {
        "RESET MASTER;"
    } else {
        "RESET BINARY LOGS AND GTIDS;"
    };
    admin(&v, reset);
    assert!(
        changes
            .next_batch(&cursor, &budget)
            .await
            .unwrap_err()
            .fact()
            .unwrap()
            .contains("rebaseline")
    );
    admin(
        &v,
        "SET GLOBAL gtid_mode='ON_PERMISSIVE'; SET GLOBAL gtid_mode='OFF_PERMISSIVE'; SET GLOBAL gtid_mode='OFF';",
    );
    let mut anonymous = engine(&v);
    let start = anonymous.watermark().await.unwrap().unwrap();
    assert!(matches!(start, Watermark::MysqlFilePos { .. }));
    let mut anonymous_changes = anonymous.changes(&tables, 9303).await.unwrap();
    writer
        .query_drop("INSERT INTO fixture.cdc_a VALUES (4000,'anonymous')")
        .await
        .unwrap();
    let batch = anonymous_changes
        .next_batch(&start, &budget)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        batch.changes,
        vec![RowChange::Insert {
            table: table("cdc_a"),
            key: vec![KeyValue::Int(4000)]
        }]
    );
    assert!(
        anonymous_changes
            .next_batch(&batch.position, &budget)
            .await
            .unwrap()
            .is_none()
    );
    anonymous_changes.close().await;
    anonymous.close().await;
    save(
        &v,
        "c1.json",
        json!({"cycles": counts, "rollback": true, "budget_retry": true, "file_position": true, "anonymous_file_position": true, "tagged": tagged, "lineage_rebaseline": true, "actual_gtid_reset": true, "purge_rebaseline": true, "typed_images": true, "empty_schema_ipc": true, "agent_db_acceptance": "pending_lane_m"}),
    );
    changes.close().await;
    file_changes.close().await;
    e.close().await;
    writer.disconnect().await.unwrap();
}

async fn repair_plans(e: &mut MysqlEngine, read: &ReadBudget) -> Vec<crate::mysql::RepairBinding> {
    use crate::mysql::{
        RepairBinding,
        c5::{Cycle, Hints},
    };
    let mut planning = read.clone();
    planning.max_chunk_rows = 40;
    let mut out = Vec::new();
    for (name, binding) in [("cdc_a", "a"), ("cdc_b", "b"), ("cdc_empty", "empty")] {
        let plan = e.plan_chunks(&table(name), &planning).await.unwrap();
        // A confirmed C5 baseline predates the edits and handoff L0.
        let mut c5 = Cycle::new(plan.chunks.clone(), 10).unwrap();
        let baseline = c5.prepare(e, read, &Hints::default()).await.unwrap();
        assert!(!baseline.replacements.is_empty());
        c5.confirm(baseline).unwrap();
        out.push(RepairBinding {
            binding_id: binding.into(),
            plan,
        });
    }
    out.push(RepairBinding {
        binding_id: "a-copy".into(),
        plan: out[0].plan.clone(),
    });
    out
}

async fn snapshot_observer(writer: &mut Conn, connection_id: u32) -> (u64, u64) {
    let thread: u64 = writer
        .exec_first(
            "SELECT THREAD_ID FROM performance_schema.threads WHERE PROCESSLIST_ID=?",
            (connection_id,),
        )
        .await
        .unwrap()
        .unwrap();
    let event: u64 = writer.exec_first("SELECT COALESCE(MAX(EVENT_ID),0) FROM performance_schema.events_statements_history_long WHERE THREAD_ID=?", (thread,)).await.unwrap().unwrap();
    (thread, event)
}

async fn concurrent_edits(writer: &mut Conn, observed: (u64, u64)) {
    // Observe the real source session, rather than racing a fixed sleep against
    // metadata/L0 setup. This instrumentation is fixture-admin-only.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let started: u64 = writer.exec_first("SELECT COUNT(*) FROM performance_schema.events_statements_history_long WHERE THREAD_ID=? AND EVENT_ID>? AND SQL_TEXT='START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY'", observed).await.unwrap().unwrap();
            if started > 0 { break; }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }).await.expect("observe first full-read transaction");
    for n in 1..=50 {
        writer.query_drop("BEGIN").await.unwrap();
        writer
            .query_drop(format!(
                "UPDATE fixture.cdc_a SET payload='concurrent-{n}' WHERE id={n}"
            ))
            .await
            .unwrap();
        writer
            .query_drop(format!("DELETE FROM fixture.cdc_a WHERE id={}", 100 + n))
            .await
            .unwrap();
        writer
            .query_drop(format!(
                "INSERT INTO fixture.cdc_a VALUES ({},'insert-{n}')",
                1000 + n
            ))
            .await
            .unwrap();
        writer
            .query_drop(format!(
                "UPDATE fixture.cdc_b SET payload='other-{n}' WHERE id={n}"
            ))
            .await
            .unwrap();
        writer.query_drop("COMMIT").await.unwrap();
        tokio::time::sleep(Duration::from_millis(3)).await;
    }
    writer
        .query_drop("UPDATE fixture.cdc_a SET id=2000,payload='key-move' WHERE id=20")
        .await
        .unwrap();
}

async fn check_repair(repair: &crate::mysql::SourceRepair, writer: &mut Conn) {
    use arrow_array::{Int32Array, StringArray};
    let mut actual = BTreeMap::new();
    for name in ["cdc_a", "cdc_b", "cdc_empty"] {
        let rows: Vec<(i32, String)> = writer
            .query(format!("SELECT id,payload FROM fixture.{name}"))
            .await
            .unwrap();
        for (id, payload) in rows {
            actual.insert((table(name), vec![KeyValue::Int(i128::from(id))]), payload);
        }
    }
    let mut staged = BTreeMap::new();
    for (key, image) in repair.rows() {
        if let Some(image) = image {
            let id = image
                .column(0)
                .as_any()
                .downcast_ref::<Int32Array>()
                .unwrap()
                .value(0);
            let payload = image
                .column(1)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap()
                .value(0)
                .to_owned();
            assert_eq!(key.1, vec![KeyValue::Int(i128::from(id))]);
            staged.insert(key.clone(), payload);
        }
    }
    assert_eq!(staged, actual, "independent SELECT vs repaired rows");
    assert!(
        !staged.contains_key(&(table("cdc_a"), vec![KeyValue::Int(500)])),
        "C5 row deleted before L0 must not survive complete repair"
    );
    assert!(repair.rows()[&(table("cdc_a"), vec![KeyValue::Int(20)])].is_none());
    assert!(repair.rows()[&(table("cdc_a"), vec![KeyValue::Int(101)])].is_none());
    assert!(repair.changes_during_reads() > 0);
    assert!(
        repair.receipts().iter().any(|r| r.low != r.high),
        "must exercise a real chunk window"
    );
    assert_eq!(repair.empty_batches()[&table("cdc_empty")].num_rows(), 0);
    assert_eq!(repair.empty_batches()[&table("cdc_empty")].num_columns(), 2);
}

#[tokio::test]
#[ignore = "two independent owned lane-C binlog fixtures"]
async fn live_handoff() {
    use crate::mysql::{HandoffBarrier, RepairBudget};
    let v = input();
    let v2 = &v["second"];
    let mut a = engine(&v);
    let mut b = engine(v2);
    a.options.source_id = "source-a".into();
    b.options.source_id = "source-b".into();
    a.options.initial_page_rows = 1;
    b.options.initial_page_rows = 1;
    let mut read = a.options.budget.clone();
    read.max_chunk_rows = 200;
    read.duty_cycle_percent = 25;
    let mut wa = super::super::super::connect(&config(&v, "writer"), &password(&v, "writer"))
        .await
        .unwrap();
    let mut wb = super::super::super::connect(&config(v2, "writer"), &password(v2, "writer"))
        .await
        .unwrap();
    let ua: String = wa
        .query_first("SELECT @@GLOBAL.server_uuid")
        .await
        .unwrap()
        .unwrap();
    let ub: String = wb
        .query_first("SELECT @@GLOBAL.server_uuid")
        .await
        .unwrap()
        .unwrap();
    assert_ne!(ua, ub, "independent real sources required");
    let (pa, pb) = tokio::join!(repair_plans(&mut a, &read), repair_plans(&mut b, &read));
    wa.query_drop("DELETE FROM fixture.cdc_a WHERE id=500")
        .await
        .unwrap();
    wb.query_drop("DELETE FROM fixture.cdc_a WHERE id=500")
        .await
        .unwrap();
    let oa = snapshot_observer(&mut wa, a.connection_id().unwrap()).await;
    let ob = snapshot_observer(&mut wb, b.connection_id().unwrap()).await;
    let staging = RepairBudget {
        max_keys: 10_000,
        max_bytes: 64 * 1024 * 1024,
        required_history_seconds: 0,
    };
    let (ra, rb, _, _) = tokio::join!(
        a.repair_to_c1(&pa, 9401, &read, staging),
        b.repair_to_c1(&pb, 9402, &read, staging),
        concurrent_edits(&mut wa, oa),
        concurrent_edits(&mut wb, ob)
    );
    let ra = ra.unwrap();
    let rb = rb.unwrap();
    assert_eq!(
        &a.watermark().await.unwrap().unwrap(),
        ra.end(),
        "writer must finish before L_end"
    );
    assert_eq!(&b.watermark().await.unwrap().unwrap(), rb.end());
    check_repair(&ra, &mut wa).await;
    check_repair(&rb, &mut wb).await;
    let count_a = ra.changes_during_reads();
    let count_b = rb.changes_during_reads();
    let scope = BTreeMap::from([
        (
            "source-a".into(),
            pa.iter().map(|p| p.binding_id.clone()).collect(),
        ),
        (
            "source-b".into(),
            pb.iter().map(|p| p.binding_id.clone()).collect(),
        ),
    ]);
    let mut barrier = HandoffBarrier::new(scope).unwrap();
    let end_a = ra.end().clone();
    let end_b = rb.end().clone();
    barrier.stage(ra).unwrap();
    assert!(barrier.confirm_applied("source-a", "a", &end_a).is_err());
    barrier.stage(rb).unwrap();
    for (source, end, plans) in [("source-a", &end_a, &pa), ("source-b", &end_b, &pb)] {
        for plan in plans {
            assert!(!barrier.ready());
            // In-memory application was compared above. This is NOT a claim
            // of durable Agent DB acceptance; lane M supplies that test.
            barrier
                .confirm_applied(source, &plan.binding_id, end)
                .unwrap();
        }
    }
    assert!(barrier.ready());
    let cursors = barrier.switch().unwrap();
    let mut ca = a
        .changes(&[table("cdc_a"), table("cdc_b"), table("cdc_empty")], 9403)
        .await
        .unwrap();
    let mut cb = b
        .changes(&[table("cdc_a"), table("cdc_b"), table("cdc_empty")], 9404)
        .await
        .unwrap();
    assert!(
        ca.next_batch(&cursors["source-a"], &read)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        cb.next_batch(&cursors["source-b"], &read)
            .await
            .unwrap()
            .is_none()
    );
    wa.query_drop("INSERT INTO fixture.cdc_a VALUES (3000,'after-switch')")
        .await
        .unwrap();
    wb.query_drop("INSERT INTO fixture.cdc_a VALUES (3000,'after-switch')")
        .await
        .unwrap();
    for (changes, cursor) in [
        (&mut ca, &cursors["source-a"]),
        (&mut cb, &cursors["source-b"]),
    ] {
        let batch = changes.next_batch(cursor, &read).await.unwrap().unwrap();
        assert_eq!(
            batch.changes,
            vec![RowChange::Insert {
                table: table("cdc_a"),
                key: vec![KeyValue::Int(3000)]
            }]
        );
    }
    // Even an exhausted bounded repair does not produce a switchable proposal.
    let tiny = RepairBudget {
        max_keys: 1,
        max_bytes: 1,
        required_history_seconds: 0,
    };
    let before = a.stats().chunks_read;
    let short_history = RepairBudget {
        required_history_seconds: 172_800,
        ..staging
    };
    assert!(
        a.repair_to_c1(&pa, 9406, &read, short_history)
            .await
            .err()
            .unwrap()
            .fact()
            .unwrap()
            .contains("new_generation_rebaseline")
    );
    assert_eq!(a.stats().chunks_read, before);
    assert_eq!(
        a.repair_to_c1(&pa, 9405, &read, tiny)
            .await
            .err()
            .unwrap()
            .kind(),
        SourceErrorKind::Budget
    );
    admin(
        &v,
        "FLUSH BINARY LOGS; PURGE BINARY LOGS BEFORE NOW() + INTERVAL 1 DAY;",
    );
    assert!(
        ca.next_batch(&cursors["source-a"], &read)
            .await
            .unwrap_err()
            .fact()
            .unwrap()
            .contains("rebaseline")
    );
    save(
        &v,
        "handoff.json",
        json!({"independent_sources":2,"bindings":8,"changes_during_reads":[count_a,count_b],"independent_select_equal":true,"tombstones":true,"chunk_window":true,"before_l0_delete":true,"barrier":true,"after_switch_c1":true,"purge_rebaseline":true,"staging_budget":true,"agent_db_acceptance":"pending_lane_m"}),
    );
    ca.close().await;
    cb.close().await;
    a.close().await;
    b.close().await;
    wa.disconnect().await.unwrap();
    wb.disconnect().await.unwrap();
}
#[test]
fn classified_ddl_parser_never_admits_statement_dml_or_ambiguous_names() {
    let table = TableRef {
        schema: "fixture".into(),
        name: "items".into(),
    };
    for sql in [
        "ALTER TABLE items ADD extra int",
        "alter table `fixture`.`items` MODIFY payload varchar(160)",
    ] {
        assert_eq!(alter_table(sql, "fixture"), Some(table.clone()));
    }
    for sql in [
        "DELETE FROM items",
        "TRUNCATE TABLE items",
        "DROP TABLE items",
        "CREATE TABLE items(id int)",
        "ALTER TABLE `odd name` ADD n int",
        "ALTER TABLE a.b.c ADD n int",
        "ALTER TABLE items",
        "/* comment */ ALTER TABLE items ADD n int",
    ] {
        assert_eq!(alter_table(sql, "fixture"), None, "{sql}");
    }
}

#[test]
fn projection_uses_proven_ordinals_and_rejects_ambiguous_history() {
    let names = |values: &[&str]| values.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let published = names(&["id", "payload"]);
    let old = names(&["id", "payload"]);
    let live = names(&["id", "payload", "extra"]);
    assert_eq!(
        projection_indexes(&published, &live, Some(&old), 2).unwrap(),
        vec![0, 1]
    );
    assert_eq!(
        projection_indexes(&published, &live, Some(&old), 3).unwrap(),
        vec![0, 1]
    );
    assert!(projection_indexes(&published, &live, Some(&old), 4).is_err());
    let moved = names(&["extra", "id", "payload"]);
    assert!(projection_indexes(&published, &moved, Some(&live), 3).is_err());
    assert_eq!(
        alter_clause("ALTER TABLE `items` MODIFY `payload` varchar(160) NOT NULL;"),
        Some("modify payload varchar(160) not null".into())
    );
}

#[test]
fn aligned_table_floor_skips_only_covered_file_events() {
    let floor = Watermark::MysqlFilePos {
        file: "bin.000010".into(),
        position: 100,
    };
    assert!(covered_file_event("bin.000009", 999, &floor).unwrap());
    assert!(covered_file_event("bin.000010", 100, &floor).unwrap());
    assert!(!covered_file_event("bin.000010", 101, &floor).unwrap());
    assert!(!covered_file_event("bin.000011", 4, &floor).unwrap());
    assert!(covered_file_event("other.000010", 4, &floor).is_err());
}
