use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use oo_connectors::engine::*;
use oo_load::IpcSink;
use oo_pipeline::{
    engine_load::{Capture, Target},
    engine_load_pg::PgTarget,
    engine_sync_pg::PgSync,
    native_arrow::{DecodedStream, decode_arrow_stream},
};
use sha2::{Digest, Sha256};
use sqlx::{
    ConnectOptions, Connection, PgConnection,
    postgres::{PgConnectOptions, PgSslMode},
};
use sqlx_core::sql_str::AssertSqlSafe;
use std::sync::Arc;

fn table() -> TableRef {
    TableRef {
        schema: "fixture".into(),
        name: "items".into(),
    }
}
fn pos(lsn: u64) -> Watermark {
    Watermark::PgLsn { lsn }
}
fn image(id: i64, payload: &str) -> RecordBatch {
    RecordBatch::try_from_iter([
        ("id", Arc::new(Int64Array::from(vec![id])) as ArrayRef),
        (
            "payload",
            Arc::new(StringArray::from(vec![payload])) as ArrayRef,
        ),
    ])
    .unwrap()
}
fn decode(batch: RecordBatch) -> (Vec<u8>, DecodedStream) {
    let mut sink = IpcSink::new(&ReadBudget::default());
    sink.push(batch).unwrap();
    let ipc = sink.finish().unwrap();
    let decoded = decode_arrow_stream(&ipc).unwrap();
    (ipc, decoded)
}
async fn connect() -> PgConnection {
    let config: serde_json::Value = serde_json::from_slice(
        &std::fs::read(std::env::var("OO_LOAD_TEST_TARGET").unwrap()).unwrap(),
    )
    .unwrap();
    let password = std::fs::read_to_string(config["password_file"].as_str().unwrap()).unwrap();
    PgConnection::connect_with(
        &PgConnectOptions::new()
            .host("127.0.0.1")
            .port(config["port"].as_u64().unwrap() as u16)
            .username("postgres")
            .database("postgres")
            .password(password.trim())
            .ssl_mode(PgSslMode::Disable)
            .disable_statement_logging(),
    )
    .await
    .unwrap()
}
async fn baseline(conn: &mut PgConnection, binding: &str) -> String {
    let t = table();
    let chunk = ChunkSpec {
        table: t.clone(),
        plan_revision: 1,
        chunk_id: 0,
        lo: None,
        hi: None,
        estimated_rows: None,
    };
    let plan = TableChunkPlan {
        protocol: ENGINE_PROTOCOL.into(),
        table: t.clone(),
        key: KeyShape::IntegerPrimaryKey {
            column: "id".into(),
        },
        strategy: ChunkStrategy::Keyset {
            columns: vec!["id".into()],
        },
        estimated_rows: None,
        chunks: vec![chunk],
    };
    let mut target = PgTarget::new(conn, binding, "baseline", &t).await.unwrap();
    if target.completed().await.unwrap().is_none() {
        target.begin(&plan).await.unwrap();
        let (ipc, decoded) = decode(image(1, "baseline"));
        target
            .stage(
                &ChunkReceipt {
                    table: t,
                    plan_revision: 1,
                    chunk_id: 0,
                    rows: 1,
                    bytes: ipc.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(&ipc)),
                    low: Some(pos(10)),
                    high: Some(pos(10)),
                },
                &decoded,
            )
            .await
            .unwrap();
        target.publish(&plan).await.unwrap();
    }
    target.relation()
}
fn transaction() -> ChangeBatch {
    ChangeBatch {
        position: pos(20),
        changes: vec![
            RowChange::Delete {
                table: table(),
                key: vec![KeyValue::Int(1)],
            },
            RowChange::Insert {
                table: table(),
                key: vec![KeyValue::Int(2)],
            },
            RowChange::Update {
                table: table(),
                key: vec![KeyValue::Int(3)],
                old_key: Some(vec![KeyValue::Int(2)]),
            },
        ],
        images: vec![
            image(1, "baseline"),
            image(2, "insert"),
            image(3, "updated"),
        ],
    }
}
fn images(batch: &ChangeBatch) -> Vec<DecodedStream> {
    batch.images.iter().cloned().map(|b| decode(b).1).collect()
}

#[tokio::test]
#[ignore = "requires an owned synthetic PostgreSQL target"]
async fn sync_target_atomic_overlay_and_resume() {
    let mut conn = connect().await;
    let relation = baseline(&mut conn, "sync-test").await;
    conn.close().await.unwrap();
    let mut conn = connect().await;
    let mut target = PgSync::open(
        &mut conn,
        "sync-test",
        "fixture-lineage",
        &[table()],
        &pos(10),
    )
    .await
    .unwrap();
    let batch = transaction();
    target
        .apply(&pos(10), &batch, &images(&batch))
        .await
        .unwrap();
    let metrics = target.metrics().await.unwrap();
    assert_eq!(metrics.0, 1);
    assert!(metrics.1 > 0);
    assert_eq!(
        target.apply(&pos(10), &batch, &images(&batch)).await,
        Err("sync_stale_position")
    );
    let mut bad = transaction();
    bad.position = pos(30);
    bad.changes.push(RowChange::Delete {
        table: TableRef {
            schema: "other".into(),
            name: "items".into(),
        },
        key: vec![KeyValue::Int(3)],
    });
    bad.images.push(image(3, "updated"));
    assert_eq!(
        target.apply(&pos(20), &bad, &images(&bad)).await,
        Err("sync_table_outside_scope")
    );
    assert_eq!(target.position().await.unwrap(), pos(20));
    assert_eq!(target.metrics().await.unwrap(), metrics);
    let mismatch = ChangeBatch {
        position: pos(30),
        changes: vec![RowChange::Insert {
            table: table(),
            key: vec![KeyValue::Int(7)],
        }],
        images: vec![image(8, "wrong-key")],
    };
    assert_eq!(
        target.apply(&pos(20), &mismatch, &images(&mismatch)).await,
        Err("sync_image_key_mismatch")
    );
    let empty = ChangeBatch {
        position: pos(30),
        changes: vec![],
        images: vec![],
    };
    target.apply(&pos(20), &empty, &[]).await.unwrap();
    assert_eq!(target.metrics().await.unwrap(), metrics);
    drop(target);
    let rows: Vec<(i64, String)> = sqlx::query_as(AssertSqlSafe(format!(
        "SELECT id,payload FROM {relation} ORDER BY id"
    )))
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert_eq!(rows, vec![(3, "updated".into())]);
    conn.close().await.unwrap();
    let mut conn = connect().await;
    assert!(
        PgSync::open(
            &mut conn,
            "sync-test",
            "wrong-lineage",
            &[table()],
            &pos(10)
        )
        .await
        .is_err()
    );
    let mut target = PgSync::open(
        &mut conn,
        "sync-test",
        "fixture-lineage",
        &[table()],
        &pos(10),
    )
    .await
    .unwrap();
    assert_eq!(target.position().await.unwrap(), pos(30));
    assert_eq!(target.metrics().await.unwrap(), metrics);
}

#[tokio::test]
#[ignore = "SIGKILL fixture driven by test-sync-target.py"]
async fn sync_target_sigkill() {
    let phase = std::env::var("OO_SYNC_PHASE").unwrap();
    let mut conn = connect().await;
    let relation = baseline(&mut conn, "sync-kill").await;
    conn.close().await.unwrap();
    let mut conn = connect().await;
    let mut target = PgSync::open(
        &mut conn,
        "sync-kill",
        "fixture-lineage",
        &[table()],
        &pos(10),
    )
    .await
    .unwrap();
    if phase == "commit" {
        let batch = transaction();
        target
            .apply(&pos(10), &batch, &images(&batch))
            .await
            .unwrap();
        std::fs::write(std::env::var("OO_SYNC_MARKER").unwrap(), "committed").unwrap();
        std::thread::sleep(std::time::Duration::from_secs(120));
    } else {
        assert_eq!(target.position().await.unwrap(), pos(20));
        assert_eq!(target.metrics().await.unwrap().0, 1);
        drop(target);
        let rows: Vec<(i64, String)> = sqlx::query_as(AssertSqlSafe(format!(
            "SELECT id,payload FROM {relation} ORDER BY id"
        )))
        .fetch_all(&mut conn)
        .await
        .unwrap();
        assert_eq!(rows, vec![(3, "updated".into())]);
    }
}

#[tokio::test]
#[ignore = "requires an owned synthetic PostgreSQL target"]
async fn c5_target_reuses_parent_and_no_change_writes_nothing() {
    let mut conn = connect().await;
    let t = table();
    let boundary = Some(vec![KeyValue::Int(2)]);
    let plan = TableChunkPlan {
        protocol: ENGINE_PROTOCOL.into(),
        table: t.clone(),
        key: KeyShape::IntegerPrimaryKey {
            column: "id".into(),
        },
        strategy: ChunkStrategy::Keyset {
            columns: vec!["id".into()],
        },
        estimated_rows: None,
        chunks: vec![
            ChunkSpec {
                table: t.clone(),
                plan_revision: 1,
                chunk_id: 0,
                lo: None,
                hi: boundary.clone(),
                estimated_rows: None,
            },
            ChunkSpec {
                table: t.clone(),
                plan_revision: 1,
                chunk_id: 1,
                lo: boundary,
                hi: None,
                estimated_rows: None,
            },
        ],
    };
    let receipt = |chunk: u32, ipc: &[u8]| ChunkReceipt {
        table: t.clone(),
        plan_revision: 1,
        chunk_id: chunk,
        rows: 1,
        bytes: ipc.len() as u64,
        sha256: format!("{:x}", Sha256::digest(ipc)),
        low: Some(pos(10)),
        high: Some(pos(10)),
    };
    let mut sink = PgTarget::new(&mut conn, "c5-test", "base", &t)
        .await
        .unwrap();
    let read_metadata = serde_json::json!({
        "consistency":"chunk_consistent",
        "chunk_bounds":{"rows":100000,"bytes":33554432,"transaction_ms":30000,"cleanup_ms":1000}
    });
    sink.read_metadata(read_metadata.clone());
    sink.begin(&plan).await.unwrap();
    for chunk in 0..2 {
        let (ipc, decoded) = decode(image(i64::from(chunk) + 1, "base"));
        sink.stage(&receipt(chunk, &ipc), &decoded).await.unwrap();
    }
    sink.publish(&plan).await.unwrap();
    assert_eq!(
        sink.publication_read_metadata().await.unwrap(),
        Some(read_metadata.clone())
    );
    // A caller changing options on resume cannot relabel the retained rows.
    sink.read_metadata(serde_json::json!({"consistency":"single"}));
    assert!(sink.completed().await.unwrap().is_some());
    assert_eq!(
        sink.publication_read_metadata().await.unwrap(),
        Some(read_metadata)
    );
    let relation = sink.relation();
    let base_relation = sink.generation_relation("base").await.unwrap().unwrap();
    let (ipc, decoded) = decode(image(1, "must-not-rewrite-sealed-storage"));
    assert_eq!(
        sink.stage(&receipt(0, &ipc), &decoded).await,
        Err("load_stage_sealed")
    );
    drop(sink);
    conn.close().await.unwrap();
    let mut conn = connect().await;
    let mut sink = PgTarget::new(&mut conn, "c5-test", "cycle-1", &t)
        .await
        .unwrap();
    let parent = sink.c5_baseline().await.unwrap();
    sink.begin(&plan).await.unwrap();
    let (ipc, decoded) = decode(image(2, "replacement"));
    let new_receipt = receipt(1, &ipc);
    sink.stage(&new_receipt, &decoded).await.unwrap();
    let digests: Vec<_> = plan
        .chunks
        .iter()
        .map(|c| RangeDigest {
            table: t.clone(),
            lo: c.lo.clone(),
            hi: c.hi.clone(),
            rows: 1,
            digest: format!("fixture-{}", c.chunk_id),
        })
        .collect();
    let mut corrupt = parent.clone();
    corrupt.parts[0].receipt.rows += 1;
    assert_eq!(
        sink.publish_c5(
            &corrupt,
            &plan,
            digests.clone(),
            std::slice::from_ref(&new_receipt),
            "fixture-lineage",
        )
        .await,
        Err("generation_part_mismatch")
    );
    assert!(sink.generation_relation("cycle-1").await.unwrap().is_none());
    sink.publish_c5(
        &parent,
        &plan,
        digests.clone(),
        &[new_receipt],
        "fixture-lineage",
    )
    .await
    .unwrap();
    assert_eq!(sink.generations().await.unwrap(), 2);
    let next = sink.c5_baseline().await.unwrap();
    assert_eq!(parent.parts[0].stage, next.parts[0].stage);
    assert_ne!(parent.parts[1].stage, next.parts[1].stage);
    drop(sink);
    let rows: Vec<(i64, String)> = sqlx::query_as(AssertSqlSafe(format!(
        "SELECT id,payload FROM {relation} ORDER BY id"
    )))
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert_eq!(rows, vec![(1, "base".into()), (2, "replacement".into())]);
    let retained: Vec<(i64, String)> = sqlx::query_as(AssertSqlSafe(format!(
        "SELECT id,payload FROM {base_relation} ORDER BY id"
    )))
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert_eq!(retained, vec![(1, "base".into()), (2, "base".into())]);
    conn.close().await.unwrap();
    let mut conn = connect().await;
    let mut sink = PgTarget::new(&mut conn, "c5-test", "cycle-2", &t)
        .await
        .unwrap();
    let previous = sink.c5_baseline().await.unwrap();
    assert_eq!(
        sink.publish_c5(&previous, &plan, digests.clone(), &[], "other-lineage")
            .await,
        Err("c5_rebaseline_required_lineage")
    );
    sink.publish_c5(&previous, &plan, digests, &[], "fixture-lineage")
        .await
        .unwrap();
    assert_eq!(sink.generations().await.unwrap(), 2);
    assert_eq!(sink.write_bytes().await.unwrap(), 0);
    assert!(sink.completed().await.unwrap().is_none());
    assert!(sink.generation_relation("cycle-2").await.unwrap().is_none());
    drop(sink);
    assert!(
        PgSync::open(&mut conn, "c5-test", "lineage", &[t], &pos(10))
            .await
            .is_err()
    );
}

#[tokio::test]
#[ignore = "requires an owned synthetic PostgreSQL target; not Gateway acceptance"]
async fn three_publications_preserve_inflight_repeatable_read() {
    let binding = "three-publications";
    let mut writer = connect().await;
    let relation = baseline(&mut writer, binding).await;
    writer.close().await.unwrap();
    let mut reader = connect().await;
    let mut observer = connect().await;
    for cycle in 1..=3 {
        sqlx::raw_sql("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut reader)
            .await
            .unwrap();
        let select = format!("SELECT payload FROM {relation}");
        let old: String = sqlx::query_scalar(AssertSqlSafe(select.clone()))
            .fetch_one(&mut reader)
            .await
            .unwrap();
        assert_eq!(
            old,
            if cycle == 1 {
                "baseline".into()
            } else {
                format!("cycle-{}", cycle - 1)
            }
        );
        let (ready, started) = tokio::sync::oneshot::channel();
        let publish = tokio::spawn(async move {
            let mut writer = connect().await;
            sqlx::raw_sql("SET statement_timeout = '10s'")
                .execute(&mut writer)
                .await
                .unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut writer)
                .await
                .unwrap();
            let mut target =
                PgTarget::new(&mut writer, binding, &format!("cycle-{cycle}"), &table())
                    .await
                    .unwrap();
            let parent = target.c5_baseline().await.unwrap();
            let plan = parent.plan.clone();
            target.begin(&plan).await.unwrap();
            let (ipc, decoded) = decode(image(1, &format!("cycle-{cycle}")));
            let receipt = ChunkReceipt {
                table: table(),
                plan_revision: plan.chunks[0].plan_revision,
                chunk_id: 0,
                rows: 1,
                bytes: ipc.len() as u64,
                sha256: format!("{:x}", Sha256::digest(&ipc)),
                low: Some(pos(10)),
                high: Some(pos(10)),
            };
            target.stage(&receipt, &decoded).await.unwrap();
            ready.send(pid).unwrap();
            target.publish(&plan).await.unwrap();
            assert_eq!(target.generations().await.unwrap(), cycle + 1);
            drop(target);
            writer.close().await.unwrap();
        });
        let pid = started.await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let blocked: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1 AND wait_event_type='Lock')",
                )
                .bind(pid)
                .fetch_one(&mut observer)
                .await
                .unwrap();
                if blocked {
                    break;
                }
                assert!(!publish.is_finished(), "publisher must wait for the pinned view");
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("publisher did not reach the reader's relation lock");
        let pinned: String = sqlx::query_scalar(AssertSqlSafe(select.clone()))
            .fetch_one(&mut reader)
            .await
            .unwrap();
        assert_eq!(pinned, old);
        sqlx::raw_sql("COMMIT").execute(&mut reader).await.unwrap();
        publish.await.unwrap();
        let current: String = sqlx::query_scalar(AssertSqlSafe(select))
            .fetch_one(&mut reader)
            .await
            .unwrap();
        assert_eq!(current, format!("cycle-{cycle}"));
    }
    let mut conn = connect().await;
    let mut target = PgTarget::new(&mut conn, binding, "inspect", &table())
        .await
        .unwrap();
    let mut retained = Vec::new();
    for run in ["baseline", "cycle-1", "cycle-2", "cycle-3"] {
        retained.push((run, target.generation_relation(run).await.unwrap().unwrap()));
    }
    drop(target);
    // New transactions can still read every old cut after all pointer switches.
    for (run, relation) in retained {
        let payload: String =
            sqlx::query_scalar(AssertSqlSafe(format!("SELECT payload FROM {relation}")))
                .fetch_one(&mut conn)
                .await
                .unwrap();
        assert_eq!(payload, run);
    }
    conn.close().await.unwrap();
    reader.close().await.unwrap();
    observer.close().await.unwrap();
}
