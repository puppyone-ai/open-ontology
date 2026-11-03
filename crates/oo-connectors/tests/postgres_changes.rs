//! Owned, opt-in PG 13-18 C1 matrix. No production endpoints.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use arrow_array::{Array, Int64Array, RecordBatch, StringArray};
use oo_connectors::engine::*;
use oo_connectors::postgres::{PostgresEngine, changes::*};
use oo_connectors::*;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection, Executor, Row};

fn config(dir: &Path, port: u16) -> PgSourceConfig {
    PgSourceConfig::try_new(
        SourceId::new("pg_changes").unwrap(),
        PgEndpoint::new("localhost", port, "postgres").unwrap(),
        PgCredentials::new(
            "oo_reader",
            std::fs::read_to_string(dir.join("private/reader")).unwrap(),
        )
        .unwrap(),
        SecretRef::new("secret://fixture/reader").unwrap(),
        PgTlsConfig::from_ca_pem(std::fs::read(dir.join("private/ca.crt")).unwrap()).unwrap(),
        PgBudget::new(
            Duration::from_secs(10),
            Duration::from_secs(60),
            Duration::from_secs(1),
            100_000,
            32 * 1024 * 1024,
        )
        .unwrap(),
        TableSelection::new(vec![
            SelectedTable::new("fixture", "items").unwrap(),
            SelectedTable::new("fixture", "root").unwrap(),
        ])
        .unwrap(),
    )
    .unwrap()
}

fn capture() -> CaptureConfig {
    CaptureConfig {
        slot: "oo_changes".into(),
        publication: "oo_changes".into(),
        required_wal_headroom: 1024 * 1024,
    }
}

fn admin(dir: &Path, port: u16) -> PgConnectOptions {
    PgConnectOptions::from_explicit()
        .host("localhost")
        .port(port)
        .database("postgres")
        .username("postgres")
        .password(&std::fs::read_to_string(dir.join("private/admin")).unwrap())
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(std::fs::read(dir.join("private/ca.crt")).unwrap())
        .disable_statement_logging()
}

fn rows(batch: &RecordBatch) -> Vec<(i64, Option<String>)> {
    let id = batch
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    let payload = batch
        .column(1)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    (0..batch.num_rows())
        .map(|i| {
            (
                id.value(i),
                (!payload.is_null(i)).then(|| payload.value(i).to_owned()),
            )
        })
        .collect()
}

fn apply(target: &mut BTreeMap<(String, i64), Option<String>>, batch: &ChangeBatch) {
    assert_eq!(batch.images.len(), batch.changes.len());
    for (change, image) in batch.changes.iter().zip(&batch.images) {
        let key = |k: &[KeyValue]| {
            let KeyValue::Int(v) = k[0] else {
                panic!("integer key")
            };
            v as i64
        };
        match change {
            RowChange::Insert { table, .. } => {
                let (id, value) = rows(image).remove(0);
                target.insert((table.name.clone(), id), value);
            }
            RowChange::Update { table, old_key, .. } => {
                if let Some(old) = old_key {
                    target.remove(&(table.name.clone(), key(old)));
                }
                let (id, value) = rows(image).remove(0);
                target.insert((table.name.clone(), id), value);
            }
            RowChange::Delete { table, key: k } => {
                target.remove(&(table.name.clone(), key(k)));
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires testing/engines/postgres/run.py --suite changes"]
async fn t1_window_t2_cycles_and_slot_lifecycle() {
    let dir = std::path::PathBuf::from(std::env::var("OO_PG_TEST_DIR").unwrap());
    let port = std::env::var("OO_PG_TEST_PORT").unwrap().parse().unwrap();
    let major: u32 = std::env::var("OO_PG_TEST_MAJOR").unwrap().parse().unwrap();
    let mut admin_conn = admin(&dir, port).connect().await.unwrap();
    let mut source = PostgresChanges::new(config(&dir, port), capture()).unwrap();
    let status = source.preflight().await.unwrap();
    assert_eq!(
        status.protocol_version,
        match major {
            13 => 1,
            14 => 2,
            15 => 3,
            _ => 4,
        }
    );
    assert!(status.publish_via_partition_root);
    let mut checkpoint = source.earliest_available().await.unwrap();
    let budget = ReadBudget::default();
    let mut engine = PostgresEngine::new(config(&dir, port));
    let table = TableRef {
        schema: "fixture".into(),
        name: "items".into(),
    };
    let plan = engine.plan_chunks(&table, &budget).await.unwrap();
    assert_eq!(plan.chunks.len(), 1);
    struct Sink {
        data: Vec<(i64, Option<String>)>,
        admin: Option<PgConnectOptions>,
    }
    impl ChunkSink for Sink {
        fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
            self.data.extend(rows(&batch));
            if let Some(options) = self.admin.take() {
                // Commit while the old snapshot buffer is still held, before H.
                std::thread::spawn(move||{
                    tokio::runtime::Runtime::new().unwrap().block_on(async move{
                        let mut conn=options.connect().await.unwrap();
                        conn.execute("BEGIN; UPDATE fixture.items SET payload='window' WHERE id=1; DELETE FROM fixture.items WHERE id=2; UPDATE fixture.items SET id=30 WHERE id=3; INSERT INTO fixture.items VALUES(4,''); COMMIT").await.unwrap();
                    });
                }).join().unwrap();
            }
            Ok(())
        }
    }
    let mut sink = Sink {
        data: Vec::new(),
        admin: Some(admin(&dir, port)),
    };
    let receipt = engine
        .read_chunk(&plan.chunks[0], &budget, &mut sink)
        .await
        .unwrap();
    assert_eq!(sink.data.len(), 3);
    let mut target: BTreeMap<_, _> = sink
        .data
        .into_iter()
        .map(|(k, v)| (("items".into(), k), v))
        .collect();
    target.insert(("root".into(), 1), Some("partition".into()));
    let tiny = ReadBudget {
        max_chunk_rows: 1,
        ..budget.clone()
    };
    assert_eq!(
        source
            .next_batch(&checkpoint, &tiny)
            .await
            .unwrap_err()
            .fact(),
        Some("capture_transaction_rows")
    );
    assert_eq!(
        source.preflight().await.unwrap().confirmed_flush_lsn,
        status.confirmed_flush_lsn
    );
    let window = source
        .next_batch(&checkpoint, &budget)
        .await
        .unwrap()
        .unwrap();
    let Watermark::PgLsn { lsn: high } = receipt.high.unwrap() else {
        panic!()
    };
    let Watermark::PgLsn { lsn: end } = window.position else {
        panic!()
    };
    assert!(end <= high);
    apply(&mut target, &window);
    checkpoint = window.position;
    assert_eq!(
        target.get(&("items".into(), 1)),
        Some(&Some("window".into()))
    );
    assert!(!target.contains_key(&("items".into(), 2)));
    assert!(!target.contains_key(&("items".into(), 3)));
    assert_eq!(target.get(&("items".into(), 30)), Some(&None));
    // Replay before durable acknowledgement is byte/value deterministic.
    let replay = source
        .next_batch(&source_position(status.earliest()), &budget)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replay.changes, window.changes);
    assert_eq!(
        source.preflight().await.unwrap().confirmed_flush_lsn,
        status.confirmed_flush_lsn
    );
    let mut oracle_checks = Vec::new();
    record_c1_oracle(&mut admin_conn, &target, "window", &mut oracle_checks).await;
    let mut cycle_counts = Vec::new();
    for cycle in 0..3 {
        if cycle == 0 {
            admin_conn.execute("BEGIN; UPDATE fixture.items SET payload=NULL WHERE id=1; UPDATE fixture.root SET payload='root-change' WHERE id=1; COMMIT").await.unwrap();
        } else if cycle == 1 {
            admin_conn.execute("BEGIN; DELETE FROM fixture.root WHERE id=1; INSERT INTO fixture.root VALUES(2,'new'); UPDATE fixture.items SET payload='three' WHERE id=30; COMMIT").await.unwrap();
        }
        let mut count = 0;
        while let Some(batch) = source.next_batch(&checkpoint, &budget).await.unwrap() {
            count += batch.changes.len();
            apply(&mut target, &batch);
            checkpoint = batch.position;
        }
        cycle_counts.push(count);
        record_c1_oracle(
            &mut admin_conn,
            &target,
            &format!("c1-{cycle}"),
            &mut oracle_checks,
        )
        .await;
    }
    assert_eq!(cycle_counts, vec![2, 3, 0]);
    std::fs::write(
        dir.join("oracle-c1.json"),
        serde_json::to_vec(&oracle_checks).unwrap(),
    )
    .unwrap();
    let c5_metrics = c5_cycles(&dir, port, &mut admin_conn).await;
    // Same name/columns, different identity: do not replay an old table's WAL
    // into a newly created target table. No automatic baseline repair here.
    admin_conn.execute("BEGIN; DROP TABLE fixture.items; CREATE TABLE fixture.items(id bigint PRIMARY KEY,payload text); GRANT SELECT ON fixture.items TO oo_reader; ALTER PUBLICATION oo_changes ADD TABLE fixture.items; COMMIT").await.unwrap();
    assert_eq!(
        source
            .next_batch(&checkpoint, &budget)
            .await
            .unwrap_err()
            .fact(),
        Some("rebaseline_required_relation_identity")
    );
    let mut missing = capture();
    missing.slot = "missing".into();
    assert_eq!(
        PostgresChanges::new(config(&dir, port), missing)
            .unwrap()
            .preflight()
            .await
            .unwrap_err()
            .fact(),
        Some("rebaseline_required_slot_missing")
    );
    let mut small = capture();
    small.required_wal_headroom = u64::MAX;
    assert_eq!(
        PostgresChanges::new(config(&dir, port), small)
            .unwrap()
            .preflight()
            .await
            .unwrap_err()
            .fact(),
        Some("wal_retention_headroom_unproven")
    );
    admin_conn
        .execute("GRANT UPDATE(payload) ON fixture.items TO oo_reader")
        .await
        .unwrap();
    assert_eq!(
        source.preflight().await.unwrap_err().fact(),
        Some("capture_table_privileges")
    );
    admin_conn
        .execute("REVOKE UPDATE(payload) ON fixture.items FROM oo_reader")
        .await
        .unwrap();
    if major >= 15 {
        admin_conn
            .execute(
                "ALTER PUBLICATION oo_changes SET TABLE fixture.items WHERE (id > 0), fixture.root",
            )
            .await
            .unwrap();
        assert_eq!(
            source.preflight().await.unwrap_err().fact(),
            Some("publication_row_filter")
        );
        admin_conn
            .execute("ALTER PUBLICATION oo_changes SET TABLE fixture.items, fixture.root")
            .await
            .unwrap();
    }
    admin_conn
        .execute("ALTER PUBLICATION oo_changes SET (publish_via_partition_root=false)")
        .await
        .unwrap();
    assert_eq!(
        source.preflight().await.unwrap_err().fact(),
        Some("publication_members_or_partition_roots")
    );
    admin_conn
        .execute("ALTER PUBLICATION oo_changes SET (publish_via_partition_root=true)")
        .await
        .unwrap();
    // Only fixture DBA advances a slot, after the target's comparison succeeds.
    let Watermark::PgLsn { lsn: end } = checkpoint else {
        panic!()
    };
    sqlx::query("SELECT pg_replication_slot_advance('oo_changes',$1::text::pg_lsn)")
        .bind(oo_connectors::postgres::lsn::format_lsn(end))
        .execute(&mut admin_conn)
        .await
        .unwrap();
    assert_eq!(
        source
            .next_batch(&source_position(status.earliest()), &budget)
            .await
            .unwrap_err()
            .fact(),
        Some("rebaseline_required_position_expired")
    );
    let idle = if major == 18 {
        admin_conn
            .execute("ALTER SYSTEM SET idle_replication_slot_timeout='1s'")
            .await
            .unwrap();
        admin_conn.execute("SELECT pg_reload_conf()").await.unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
        admin_conn.execute("CHECKPOINT").await.unwrap();
        assert_eq!(
            source.preflight().await.unwrap_err().fact(),
            Some("rebaseline_required_slot_invalidated")
        );
        let reason: String = sqlx::query_scalar(
            "SELECT invalidation_reason FROM pg_replication_slots WHERE slot_name='oo_changes'",
        )
        .fetch_one(&mut admin_conn)
        .await
        .unwrap();
        assert_eq!(reason, "idle_timeout");
        Some(reason)
    } else {
        None
    };
    std::fs::write(
        dir.join("T2.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "major":major,"proto_version":status.protocol_version,"window_correction":true,
            "c1_cycle_events":cycle_counts,"idle_invalidation":idle,"product_slot_ack":false,
            "c5_in_memory_target_metrics":c5_metrics,"recreated_relation_refused":true,
            "over_budget_transaction_refused_without_ack":true
        }))
        .unwrap(),
    )
    .unwrap();
    admin_conn.close().await.unwrap();
}

fn source_position(lsn: u64) -> Watermark {
    Watermark::PgLsn { lsn }
}

async fn record_c1_oracle(
    admin: &mut sqlx::PgConnection,
    target: &BTreeMap<(String, i64), Option<String>>,
    name: &str,
    checks: &mut Vec<serde_json::Value>,
) {
    let gold = sqlx::query("SELECT 'items' AS t,id,payload FROM fixture.items UNION ALL SELECT 'root',id,payload FROM fixture.root ORDER BY t,id")
        .fetch_all(admin).await.unwrap();
    let expected: BTreeMap<_, _> = gold
        .iter()
        .map(|r| {
            (
                (r.get::<String, _>(0), r.get::<i64, _>(1)),
                r.get::<Option<String>, _>(2),
            )
        })
        .collect();
    assert_eq!(*target, expected);
    // Source orders the business keys. The fixture's only textual key is the
    // fixed ASCII table label ("items"/"root"), not a collated business value.
    checks.push(serde_json::json!({
        "name":name, "key_columns":[0,1],
        "source":gold.iter().map(|r|serde_json::json!([
            r.get::<String,_>(0),r.get::<i64,_>(1),r.get::<Option<String>,_>(2)
        ])).collect::<Vec<_>>(),
        "target":target.iter().map(|((table,id),value)|serde_json::json!([table,id,value])).collect::<Vec<_>>()
    }));
}

async fn c5_cycles(
    dir: &Path,
    port: u16,
    admin: &mut sqlx::PgConnection,
) -> Vec<serde_json::Value> {
    use oo_connectors::postgres::reconcile::C5Cycle;
    let mut engine = PostgresEngine::new(config(dir, port));
    let table = TableRef {
        schema: "fixture".into(),
        name: "items".into(),
    };
    let budget = ReadBudget {
        // Planner reserves half the row budget for growth: target two rows.
        max_chunk_rows: 4,
        ..ReadBudget::default()
    };
    let plan = engine.plan_chunks(&table, &budget).await.unwrap();
    assert_eq!(plan.chunks.len(), 2);
    let mut previous = Vec::new();
    let mut target = BTreeMap::new();
    let mut metrics = Vec::new();
    let mut oracle_checks = Vec::new();
    struct Sink(Vec<(i64, Option<String>)>);
    impl ChunkSink for Sink {
        fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
            self.0.extend(rows(&batch));
            Ok(())
        }
    }
    for cycle in 1..=4 {
        if cycle == 2 {
            // Same COUNT; NULL changes without a reliable timestamp hint.
            admin
                .execute("UPDATE fixture.items SET payload='c5-null-change' WHERE id=1")
                .await
                .unwrap();
        } else if cycle == 3 {
            admin.execute("BEGIN; DELETE FROM fixture.items WHERE id=4; INSERT INTO fixture.items VALUES(5,NULL); COMMIT").await.unwrap();
        }
        let policy = C5Cycle {
            cycle,
            full_read_every: 100,
            positive_hints: Default::default(),
            recent_chunks: [plan.chunks[1].chunk_id].into_iter().collect(),
        };
        let inspection = engine.inspect_c5(&table, &previous, &policy).await.unwrap();
        assert_eq!(inspection.source_digests.len(), 2); // Recent is not exhaustive.
        assert_eq!(inspection.source_digests[0].lo, plan.chunks[1].lo);
        assert!(!inspection.full_read_due);
        let mut payload = 0;
        let mut rewritten = 0;
        let bound = |v: &Option<Vec<KeyValue>>| {
            v.as_ref().map(|keys| {
                let KeyValue::Int(id) = keys[0] else {
                    panic!("integer fixture")
                };
                id as i64
            })
        };
        for chunk in &inspection.read_chunks {
            let mut sink = Sink(Vec::new());
            let receipt = engine.read_chunk(chunk, &budget, &mut sink).await.unwrap();
            payload += receipt.bytes;
            rewritten += receipt.rows;
            target.retain(|id, _| {
                bound(&chunk.lo).is_some_and(|lo| *id < lo)
                    || bound(&chunk.hi).is_some_and(|hi| *id >= hi)
            });
            target.extend(sink.0);
        }
        let gold = sqlx::query("SELECT id,payload FROM fixture.items ORDER BY id")
            .fetch_all(&mut *admin)
            .await
            .unwrap();
        let expected: BTreeMap<_, _> = gold
            .iter()
            .map(|r| (r.get::<i64, _>(0), r.get::<Option<String>, _>(1)))
            .collect();
        assert_eq!(target, expected);
        oracle_checks.push(serde_json::json!({
            "name":format!("c5-{cycle}"),"key_columns":[0],
            "source":gold.iter().map(|r|serde_json::json!([
                r.get::<i64,_>(0),r.get::<Option<String>,_>(1)
            ])).collect::<Vec<_>>(),
            "target":target.iter().map(|(id,value)|serde_json::json!([id,value])).collect::<Vec<_>>()
        }));
        if cycle == 4 {
            assert!(inspection.read_chunks.is_empty());
            assert_eq!((payload, rewritten), (0, 0));
        } else {
            assert!(payload > 0 && rewritten > 0);
        }
        metrics.push(serde_json::json!({
            "cycle":cycle, "payload_bytes":payload, "rewritten_rows":rewritten,
            "generation_requested":!inspection.read_chunks.is_empty()
        }));
        previous = inspection.source_digests;
    }
    let forced = C5Cycle {
        cycle: 5,
        full_read_every: 100,
        positive_hints: [plan.chunks[0].chunk_id].into_iter().collect(),
        recent_chunks: Default::default(),
    };
    assert_eq!(
        engine
            .inspect_c5(&table, &previous, &forced)
            .await
            .unwrap()
            .read_chunks
            .len(),
        1
    );
    let fallback = C5Cycle {
        cycle: 100,
        positive_hints: Default::default(),
        ..forced
    };
    let fallback = engine
        .inspect_c5(&table, &previous, &fallback)
        .await
        .unwrap();
    assert!(fallback.full_read_due);
    assert_eq!(fallback.read_chunks.len(), 2);
    std::fs::write(
        dir.join("oracle-c5.json"),
        serde_json::to_vec(&oracle_checks).unwrap(),
    )
    .unwrap();
    metrics
}
