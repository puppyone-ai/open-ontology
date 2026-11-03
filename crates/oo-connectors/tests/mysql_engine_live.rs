//! Lane-C owned fixtures only. The runner creates accounts, TLS and data.
#![cfg(feature = "mysql")]

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use arrow_array::RecordBatch;
use mysql_async::{Conn, prelude::Queryable};
use oo_connectors::engine::{
    ChunkReceipt, ChunkSink, ChunkSpec, ChunkStrategy, KeyShape, KeyValue, ReadBudget,
    SourceEngine, TableRef, Watermark,
};
use oo_connectors::mysql::{
    EngineOptions, MysqlConfig, MysqlEngine, SelectedTable, catalog_columns, connect,
    engine::{RowFingerprint, key_value},
    gtid::GtidSet,
    keyset::KeyKind,
    map::map_capture_column,
    raw_cell,
    values::decode_mapped,
};
use oo_connectors::types::{RunBinding, assemble_batch};
use oo_connectors::{SecretRef, SourceError};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

#[path = "mysql_binlog_fixture.rs"]
mod binlog_fixture;

#[derive(Default)]
struct Sink(Vec<RecordBatch>);
impl ChunkSink for Sink {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        self.0.push(batch);
        Ok(())
    }
}

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
        secret_ref: SecretRef::new("secret://lane-c-fixture").unwrap(),
        connect_timeout_ms: 5000,
    }
}

fn password(v: &Json, user: &str) -> Zeroizing<String> {
    Zeroizing::new(fs::read_to_string(v[format!("{user}_password")].as_str().unwrap()).unwrap())
}

fn engine(v: &Json, options: EngineOptions) -> MysqlEngine {
    MysqlEngine::new(config(v, "reader"), password(v, "reader"), options)
}

fn save(v: &Json, name: &str, body: &Json) {
    fs::write(
        Path::new(v["out"].as_str().unwrap()).join(name),
        serde_json::to_vec_pretty(body).unwrap(),
    )
    .unwrap();
}

fn check_ipc(sink: &Sink, receipt: &ChunkReceipt) {
    let mut ipc =
        arrow_ipc::writer::StreamWriter::try_new(Vec::new(), &sink.0[0].schema()).unwrap();
    for batch in &sink.0 {
        ipc.write(batch).unwrap();
    }
    ipc.finish().unwrap();
    let bytes = ipc.into_inner().unwrap();
    assert_eq!(receipt.bytes, bytes.len() as u64);
    assert_eq!(receipt.sha256, format!("{:x}", Sha256::digest(bytes)));
    if let (
        Some(Watermark::MysqlGtidSet { gtid_set: lo }),
        Some(Watermark::MysqlGtidSet { gtid_set: hi }),
    ) = (&receipt.low, &receipt.high)
    {
        assert!(
            GtidSet::parse(hi)
                .unwrap()
                .contains(&GtidSet::parse(lo).unwrap())
        );
    } else {
        panic!("GTID fixture must return real low/high positions");
    }
}

// Independent full SELECT, without the planner, predicates, or adapter paging.
// The common type encoder supplies the project's canonical row hash definition.
async fn oracle(conn: &mut Conn, name: &str, keys: &[(&str, KeyKind)]) -> Vec<RowFingerprint> {
    let selected = SelectedTable {
        schema: "fixture".into(),
        name: name.into(),
        columns: vec![],
    };
    let catalog = catalog_columns(conn, &mut vec![], &selected).await.unwrap();
    let columns = catalog
        .iter()
        .map(|c| {
            let mut m = map_capture_column(c, false, true, &Default::default()).unwrap();
            m.key = keys.iter().any(|(k, _)| *k == c.name);
            m.plan.key = m.key;
            m
        })
        .collect::<Vec<_>>();
    let binding = RunBinding {
        source_id: "mysql".into(),
        schema: "fixture".into(),
        table: name.into(),
        run_generation: 0,
        input_digest: Sha256::digest(format!("fixture.{name}")).into(),
    };
    let order = if keys.is_empty() {
        String::new()
    } else {
        format!(
            " ORDER BY {}",
            keys.iter()
                .map(|(key, _)| format!("`{key}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let mut result = conn
        .query_iter(format!("SELECT * FROM fixture.`{name}`{order}"))
        .await
        .unwrap();
    let mut answer = Vec::new();
    let mut rows = Vec::new();
    loop {
        let next = result.next().await.unwrap();
        let ended = next.is_none();
        if let Some(row) = next {
            rows.push(row);
        }
        if rows.len() >= 5000 || ended {
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
            let batch = assemble_batch(
                &binding,
                columns.iter().map(|c| c.plan.clone()).collect(),
                &columns
                    .iter()
                    .filter(|c| c.key)
                    .map(|c| c.plan.attnum)
                    .collect::<Vec<_>>(),
                decoded,
            )
            .unwrap();
            for (row, typed) in rows.iter().zip(batch.rows) {
                let key = keys
                    .iter()
                    .map(|(k, kind)| {
                        let i = catalog.iter().position(|c| c.name == *k).unwrap();
                        key_value(*kind, row.as_ref(i).unwrap()).unwrap()
                    })
                    .collect();
                answer.push(RowFingerprint {
                    key,
                    canonical_hash: typed.canonical_hash,
                });
            }
            rows.clear();
        }
        if ended {
            break;
        }
    }
    result.drop_result().await.unwrap();
    answer
}

#[tokio::test]
#[ignore = "requires the shared fixture via run.py --suite s2-digest"]
async fn digest_primitives() {
    let path = std::env::var("OO_MYSQL_ENGINE_FIXTURE").expect("owned fixture config");
    let v: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let mut admin = connect(&config(&v, "writer"), &password(&v, "writer"))
        .await
        .unwrap();
    let budget = ReadBudget {
        max_chunk_rows: 97,
        ..Default::default()
    };
    let mut e = engine(
        &v,
        EngineOptions {
            budget: budget.clone(),
            ..Default::default()
        },
    );
    // Equal duplicate pairs must not cancel, even when the count is unchanged.
    admin
        .query_drop("DELETE FROM fixture.no_key")
        .await
        .unwrap();
    admin
        .query_drop("INSERT INTO fixture.no_key VALUES (1,'old'),(1,'old'),(2,NULL),(2,NULL)")
        .await
        .unwrap();
    let before = e.digest_range(&table("no_key"), None, None).await.unwrap();
    admin
        .query_drop("UPDATE fixture.no_key SET note='new' WHERE value_no=1")
        .await
        .unwrap();
    let after = e.digest_range(&table("no_key"), None, None).await.unwrap();
    assert_eq!(before.rows, after.rows);
    assert_ne!(before.digest, after.digest);

    let first = e.digest_range(&table("events"), None, None).await.unwrap();
    // Internal partition sizes must not change the digest.
    let mut whole = engine(&v, EngineOptions::default());
    assert_eq!(
        first,
        whole
            .digest_range(&table("events"), None, None)
            .await
            .unwrap()
    );
    whole.close().await;
    admin
        .query_drop("UPDATE fixture.events SET event_no=event_no+1 WHERE id=1")
        .await
        .unwrap();
    admin
        .query_drop("DELETE FROM fixture.events WHERE id=2")
        .await
        .unwrap();
    admin
        .query_drop("INSERT INTO fixture.events VALUES (2001,1,0)")
        .await
        .unwrap();
    let second = e.digest_range(&table("events"), None, None).await.unwrap();
    assert_eq!(first.rows, second.rows);
    assert_ne!(first.digest, second.digest);
    let third = e.digest_range(&table("events"), None, None).await.unwrap();
    assert_eq!(second, third);
    let plan = e.plan_chunks(&table("events"), &budget).await.unwrap();
    let (actual, _) = capture(&mut e, plan.chunks, &budget).await;
    let expected = oracle(&mut admin, "events", &[("id", KeyKind::Int)]).await;
    assert_eq!(actual, expected);
    let wire = |rows: &[RowFingerprint]| {
        rows.iter()
            .map(|r| {
                let KeyValue::Int(id) = r.key[0] else {
                    panic!("integer fixture key")
                };
                json!([
                    id,
                    r.canonical_hash
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                ])
            })
            .collect::<Vec<_>>()
    };
    save(
        &v,
        "digest-oracle.json",
        &json!({"source":wire(&expected),"adapter":wire(&actual)}),
    );
    save(
        &v,
        "digest-primitives.json",
        &json!({"duplicate_before":before,"duplicate_after":after,
            "cycles":[first,second,third],"rows":actual.len(),"canonical_diff":0,
            "scope":"source primitives only; no target, generation or C5 scheduler"}),
    );
    e.close().await;
    admin.disconnect().await.unwrap();
}

async fn capture(
    e: &mut MysqlEngine,
    chunks: Vec<ChunkSpec>,
    budget: &ReadBudget,
) -> (Vec<RowFingerprint>, Vec<ChunkReceipt>) {
    let mut pending: VecDeque<_> = chunks.into();
    let mut rows = Vec::new();
    let mut receipts = Vec::new();
    let mut next_id = 1_000_000;
    while let Some(chunk) = pending.pop_front() {
        let mut sink = Sink::default();
        match e.read_chunk_with_rows(&chunk, budget, &mut sink).await {
            Ok((r, part)) => {
                check_ipc(&sink, &r);
                assert_eq!(r.rows, part.len() as u64);
                assert!(r.rows <= budget.max_chunk_rows);
                assert!(r.bytes <= budget.max_chunk_bytes);
                receipts.push(r);
                rows.extend(part);
            }
            Err(err) if err.fact().is_some_and(|s| s.contains("over_budget")) => {
                assert!(sink.0.is_empty());
                let children = e.split_chunk(&chunk, budget, next_id).await.unwrap();
                assert!(children.len() > 1, "split must progress");
                assert_eq!(children.first().unwrap().lo, chunk.lo);
                assert_eq!(children.last().unwrap().hi, chunk.hi);
                assert!(
                    children
                        .iter()
                        .all(|c| c.plan_revision == chunk.plan_revision + 1)
                );
                next_id += children.len() as u32;
                // Finish the split range before later ranges, retaining source order.
                for child in children.into_iter().rev() {
                    pending.push_front(child);
                }
            }
            Err(e) => panic!("chunk read failed: {e}"),
        }
    }
    (rows, receipts)
}

fn multiset(rows: &[RowFingerprint]) -> HashMap<RowFingerprint, usize> {
    let mut out = HashMap::new();
    for row in rows {
        *out.entry(row.clone()).or_default() += 1;
    }
    out
}

fn root_sql(v: &Json, sql: &str) -> String {
    let out = Command::new("docker")
        .args([
            "exec",
            v["container"].as_str().unwrap(),
            "mysql",
            "--defaults-extra-file=/fixture/admin.cnf",
            "-Nse",
            sql,
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "fixture admin command failed");
    String::from_utf8(out.stdout).unwrap()
}

#[tokio::test]
#[ignore = "requires testing/engines/mysql/run.py owned disposable fixture"]
async fn engine_matrix_contract() {
    let path = std::env::var("OO_MYSQL_ENGINE_FIXTURE").expect("owned fixture config");
    let v: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let budget = ReadBudget {
        max_chunk_rows: 50_000,
        ..ReadBudget::default()
    };
    let mut e = engine(
        &v,
        EngineOptions {
            budget: budget.clone(),
            ..Default::default()
        },
    );
    let report = e.probe().await.unwrap();
    assert_eq!(report.version.to_string(), v["version"].as_str().unwrap());
    assert!(
        report.requirements().is_empty(),
        "{:?}",
        report.requirements()
    );
    save(&v, "t0.json", &json!(report));
    let mut admin = connect(&config(&v, "writer"), &password(&v, "writer"))
        .await
        .unwrap();
    let monitoring = Arc::new(AtomicBool::new(true));
    let flag = monitoring.clone();
    let monitor_config = config(&v, "writer");
    let monitor_password = password(&v, "writer");
    let monitor = tokio::spawn(async move {
        let mut conn = connect(&monitor_config, &monitor_password).await.unwrap();
        let mut peak = 0;
        while flag.load(Ordering::Relaxed) {
            let rows: Vec<mysql_async::Row> = conn.query("SHOW PROCESSLIST").await.unwrap();
            peak = peak.max(
                rows.iter()
                    .filter(|r| r.get::<String, _>("User").as_deref() == Some("reader"))
                    .count(),
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        conn.disconnect().await.unwrap();
        peak
    });

    let fixtures: &[(&str, &[(&str, KeyKind)])] = &[
        ("text_rows", &[("k", KeyKind::Text)]),
        (
            "composite_rows",
            &[("tenant", KeyKind::Text), ("seq", KeyKind::Int)],
        ),
        ("unique_rows", &[("code", KeyKind::Text)]),
        ("no_key", &[]),
        ("empty_rows", &[("id", KeyKind::Int)]),
        ("scale_rows", &[("id", KeyKind::Int)]),
    ];
    let mut comparisons = Vec::new();
    for (name, keys) in fixtures {
        let b = if *name == "scale_rows" {
            budget.clone()
        } else {
            ReadBudget {
                max_chunk_rows: if *name == "no_key" { 10 } else { 3 },
                ..budget.clone()
            }
        };
        let plan = e.plan_chunks(&table(name), &b).await.unwrap();
        match *name {
            "text_rows" => assert!(
                matches!(plan.key, KeyShape::StringPrimaryKey { collation: Some(ref c), .. } if c == "utf8mb4_general_ci")
            ),
            "composite_rows" => assert!(matches!(plan.key, KeyShape::CompositePrimaryKey { .. })),
            "unique_rows" => assert!(matches!(plan.key, KeyShape::UniqueIndex { .. })),
            "no_key" => assert_eq!(plan.strategy, ChunkStrategy::SingleRead),
            _ => {}
        }
        let gold = oracle(&mut admin, name, keys).await;
        let (actual, receipts) = capture(&mut e, plan.chunks, &b).await;
        assert_eq!(actual.len(), gold.len(), "{name} count");
        assert_eq!(multiset(&actual), multiset(&gold), "{name} row hashes");
        if *name == "scale_rows" {
            assert_eq!(gold.len(), 2_000_000);
        }
        let digest = e.digest_range(&table(name), None, None).await.unwrap();
        assert_eq!(digest.rows as usize, gold.len());
        comparisons.push(json!({"table":name,"rows":gold.len(),"diff":0,"chunks":receipts}));
        println!(
            "{name}: {} rows, zero canonical hash difference",
            gold.len()
        );
    }
    save(&v, "static-reads.json", &json!(comparisons));

    // A value containing the CONCAT_WS separator must not collide with
    // the same separator shifted to a neighboring column.
    let d1 = e
        .digest_range(&table("digest_edges"), None, None)
        .await
        .unwrap();
    admin
        .query_drop("UPDATE fixture.digest_edges SET a=0x61,b=0x621f63")
        .await
        .unwrap();
    let d2 = e
        .digest_range(&table("digest_edges"), None, None)
        .await
        .unwrap();
    assert_ne!(d1.digest, d2.digest);

    let growing_budget = ReadBudget {
        max_chunk_rows: 50,
        ..budget.clone()
    };
    let plan = e
        .plan_chunks(&table("growing"), &growing_budget)
        .await
        .unwrap();
    admin
        .query_drop(
            "INSERT INTO fixture.growing SELECT * FROM fixture.scale_rows WHERE id>40 AND id<=151",
        )
        .await
        .unwrap();
    let (grown, receipts) = capture(&mut e, plan.chunks, &growing_budget).await;
    assert_eq!(grown.len(), 151);
    assert!(receipts.iter().any(|r| r.plan_revision > 0));
    assert_eq!(
        multiset(&grown),
        multiset(&oracle(&mut admin, "growing", &[("id", KeyKind::Int)]).await)
    );
    save(&v, "growth.json", &json!(receipts));

    // Force the sampling branch on a small fixture; thresholds/rate are
    // explicit test inputs, while the product defaults remain 1000/1000.
    e.close().await;
    let mut sampled = engine(
        &v,
        EngineOptions {
            sample_threshold_chunks: 2,
            sample_rate: 2,
            ..Default::default()
        },
    );
    let sample_budget = ReadBudget {
        max_chunk_rows: 10,
        ..budget.clone()
    };
    let sample_plan = sampled
        .plan_chunks(&table("composite_rows"), &sample_budget)
        .await
        .unwrap();
    assert!(sample_plan.chunks.len() > 1);
    let (sample_rows, _) = capture(&mut sampled, sample_plan.chunks, &sample_budget).await;
    assert_eq!(
        multiset(&sample_rows),
        multiset(
            &oracle(
                &mut admin,
                "composite_rows",
                &[("tenant", KeyKind::Text), ("seq", KeyKind::Int)]
            )
            .await
        )
    );
    sampled.close().await;

    // Real binlog-window correction in the harness (not a product CDC reader).
    let b = ReadBudget {
        max_chunk_rows: 100,
        ..budget.clone()
    };
    let plan = e.plan_chunks(&table("int_rows"), &b).await.unwrap();
    let comparison_ranges = plan.chunks.clone();
    let status_sql = report.settings["status_statement"].as_str();
    let start = root_sql(&v, status_sql);
    let fields = start.split_whitespace().collect::<Vec<_>>();
    let file = fields[0].to_owned();
    let pos = fields[1].parse::<u64>().unwrap();
    let low = e.watermark().await.unwrap().unwrap();
    let writer_config = config(&v, "writer");
    let writer_password = password(&v, "writer");
    let writer = tokio::spawn(async move {
        let mut conn = connect(&writer_config, &writer_password).await.unwrap();
        for i in 1..=100 {
            conn.query_drop("START TRANSACTION").await.unwrap();
            for sql in [
                format!("UPDATE fixture.int_rows SET payload='updated-{i}' WHERE id={i}"),
                format!("DELETE FROM fixture.int_rows WHERE id={}", 1000 + i),
                format!(
                    "INSERT INTO fixture.int_rows VALUES ({},'inserted-{i}')",
                    3000 + i
                ),
                format!(
                    "UPDATE fixture.int_rows SET id={} WHERE id={}",
                    4000 + i,
                    1500 + i
                ),
            ] {
                conn.query_drop(sql).await.unwrap();
            }
            conn.query_drop("COMMIT").await.unwrap();
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        conn.disconnect().await.unwrap();
    });
    let (captured, receipts) = capture(&mut e, plan.chunks, &b).await;
    assert_ne!(
        receipts.first().unwrap().low,
        receipts.last().unwrap().high,
        "writer must commit during the chunked read window"
    );
    writer.await.unwrap();
    let high = e.watermark().await.unwrap().unwrap();
    assert_ne!(low, high);
    assert_eq!(
        root_sql(&v, status_sql).split_whitespace().next().unwrap(),
        file,
        "fixture binlog unexpectedly rotated"
    );
    let output = Command::new("docker")
        .args([
            "exec",
            v["container"].as_str().unwrap(),
            "tail",
            "-c",
            &format!("+{}", pos + 1),
            &format!("/var/lib/mysql/{file}"),
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "fixture binlog read failed");
    fs::write(
        Path::new(v["out"].as_str().unwrap()).join("writer-window.binlog"),
        &output.stdout,
    )
    .unwrap();
    let affected = binlog_fixture::affected_keys(&output.stdout);
    assert_eq!(
        affected.len(),
        500,
        "every insert/update/delete and both PK images"
    );
    let mut corrected: HashMap<_, _> = captured.into_iter().map(|r| (r.key.clone(), r)).collect();
    let keys: Vec<_> = affected
        .into_iter()
        .map(|id| vec![KeyValue::Int(id)])
        .collect();
    for key in &keys {
        corrected.remove(key);
    }
    for row in e.read_keys(&table("int_rows"), &keys).await.unwrap() {
        corrected.insert(row.key.clone(), row);
    }
    let gold = oracle(&mut admin, "int_rows", &[("id", KeyKind::Int)]).await;
    let gold_by_key: HashMap<_, _> = gold
        .iter()
        .map(|r| (r.key.clone(), r.canonical_hash))
        .collect();
    let mut chunk_comparisons = Vec::new();
    for range in comparison_ranges {
        let mut terms = Vec::new();
        let mut values = Vec::new();
        if let Some(lo) = &range.lo {
            terms.push("id >= ?");
            values.push(oo_connectors::mysql::engine::key_param(&lo[0]).unwrap());
        }
        if let Some(hi) = &range.hi {
            terms.push("id < ?");
            values.push(oo_connectors::mysql::engine::key_param(&hi[0]).unwrap());
        }
        let predicate = if terms.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", terms.join(" AND "))
        };
        let ids: Vec<i32> = admin
            .exec(
                format!("SELECT id FROM fixture.int_rows{predicate} ORDER BY id"),
                values,
            )
            .await
            .unwrap();
        let (mut source_hash, mut target_hash) = (Sha256::new(), Sha256::new());
        for id in &ids {
            let key = vec![KeyValue::Int(i128::from(*id))];
            source_hash.update(gold_by_key[&key]);
            target_hash.update(corrected[&key].canonical_hash);
        }
        let source_hash = format!("{:x}", source_hash.finalize());
        let target_hash = format!("{:x}", target_hash.finalize());
        assert_eq!(source_hash, target_hash);
        chunk_comparisons.push(json!({"range":range,"rows":ids.len(),
            "source_canonical_sha256":source_hash,"corrected_canonical_sha256":target_hash,"diff":0}));
    }
    assert_eq!(
        multiset(&corrected.into_values().collect::<Vec<_>>()),
        multiset(&gold)
    );
    assert_eq!(
        e.watermark().await.unwrap(),
        Some(high.clone()),
        "correction needs a quiescent end cut"
    );
    save(
        &v,
        "t1-window.json",
        &json!({
            "mode":"harness_binlog_keys_then_quiescent_point_read",
            "product_change_source":false,"rows":gold.len(),"diff":0,
            "low":low,"high":high,"affected_keys":keys.len(),"chunks":receipts
            ,"chunk_comparisons":chunk_comparisons
        }),
    );
    e.close().await;
    monitoring.store(false, Ordering::Relaxed);
    let observed_peak = monitor.await.unwrap();
    assert_eq!(observed_peak, 1);
    let stats = e.stats();
    assert_eq!(stats.peak_connections, 1);
    assert!(stats.rest_ms + 100 >= stats.busy_ms * 3, "{stats:?}");
    save(
        &v,
        "t8.json",
        &json!({"stats":stats,"wall_ms":e.wall().as_millis(),
        "observed_peak_reader_connections":observed_peak,
        "scope":"adapter only; admin/writer connections excluded"}),
    );
    admin.disconnect().await.unwrap();

    let mut options = EngineOptions::default();
    options.protection.threads_running_max = 0;
    options.protection.pause_step = Duration::from_millis(10);
    options.protection.max_pause = Duration::from_millis(20);
    let mut paused = engine(&v, options);
    assert_eq!(
        paused.probe().await.unwrap_err().fact(),
        Some("source_busy")
    );
    assert!(paused.stats().pauses >= 2);
    paused.close().await;

    // Missing replication privileges produce DBA statements, not false success.
    root_sql(
        &v,
        "REVOKE REPLICATION SLAVE, REPLICATION CLIENT ON *.* FROM 'reader'@'%'",
    );
    let mut missing = engine(&v, EngineOptions::default());
    let p = missing.probe().await.unwrap();
    assert!(p.requirements().iter().any(|r| {
        r.name == "replication_slave"
            && r.dba_statement
                .as_ref()
                .unwrap()
                .starts_with("GRANT REPLICATION SLAVE")
    }));
    save(&v, "t0-missing-grants.json", &json!(p));
    missing.close().await;
    root_sql(&v, "REVOKE SELECT ON fixture.* FROM 'reader'@'%'");
    let mut no_select = engine(&v, EngineOptions::default());
    let p = no_select.probe().await.unwrap();
    assert!(p.requirements().iter().any(|r| r.name == "select"
        && r.dba_statement.as_deref() == Some("GRANT SELECT ON `fixture`.* TO 'reader'@'%';")));
    save(&v, "t0-missing-select.json", &json!(p));
    no_select.close().await;
    root_sql(&v, "GRANT SELECT ON fixture.* TO 'reader'@'%'");
    root_sql(
        &v,
        "GRANT REPLICATION SLAVE, REPLICATION CLIENT ON *.* TO 'reader'@'%'",
    );
    if report.version.major >= 9 || report.version.minor >= 4 && report.version.major == 8 {
        root_sql(
            &v,
            "SET SESSION gtid_next='AUTOMATIC:lane_c'; BEGIN; UPDATE fixture.int_rows SET payload='tagged' WHERE id=1; COMMIT;",
        );
        let mut tagged = engine(&v, EngineOptions::default());
        let Some(Watermark::MysqlGtidSet { gtid_set }) = tagged.watermark().await.unwrap() else {
            panic!("tagged GTID watermark missing");
        };
        assert!(GtidSet::parse(&gtid_set).unwrap().has_tags());
        save(&v, "tagged-gtid.json", &json!({"gtid_set":gtid_set}));
        tagged.close().await;
    }
    root_sql(
        &v,
        "SET GLOBAL gtid_mode=ON_PERMISSIVE; SET GLOBAL gtid_mode=OFF_PERMISSIVE; SET GLOBAL gtid_mode=OFF;",
    );
    let mut filepos = engine(&v, EngineOptions::default());
    let wm = filepos.watermark().await.unwrap();
    assert!(matches!(wm,Some(Watermark::MysqlFilePos {position,..}) if position>=4));
    save(&v, "file-position.json", &json!(wm));
    filepos.close().await;
}
