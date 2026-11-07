//! Direct, read-only SourceEngine checks. These are not Agent DB acceptance.
#![cfg(any(feature = "mongodb", feature = "opensearch"))]

use arrow_array::{RecordBatch, StringArray};
use oo_connectors::{
    SourceError,
    document::{ScalarColumn, ScalarType},
    engine::*,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Default)]
struct Sink(Vec<RecordBatch>);
impl ChunkSink for Sink {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        self.0.push(batch);
        Ok(())
    }
}
impl Sink {
    fn rows(&self) -> BTreeMap<String, Value> {
        let mut out = BTreeMap::new();
        for batch in &self.0 {
            let ids = batch
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let docs = batch
                .column(batch.num_columns() - 1)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            for i in 0..batch.num_rows() {
                assert!(
                    out.insert(
                        ids.value(i).into(),
                        serde_json::from_str(docs.value(i)).unwrap()
                    )
                    .is_none(),
                    "duplicate document across chunks"
                );
            }
        }
        out
    }
}
fn fixture() -> Value {
    serde_json::from_slice(
        &std::fs::read(
            std::env::var("OO_DOCUMENT_LIVE_CONFIG").expect("explicit isolated fixture config"),
        )
        .unwrap(),
    )
    .unwrap()
}
fn secret(v: &Value, name: &str) -> String {
    std::fs::read_to_string(v[name].as_str().unwrap()).unwrap()
}
fn budget() -> ReadBudget {
    ReadBudget {
        max_chunk_rows: 64,
        max_chunk_bytes: 1024 * 1024,
        statement_timeout_ms: 15_000,
        ..ReadBudget::default()
    }
}
fn columns() -> Vec<ScalarColumn> {
    vec![ScalarColumn {
        name: "value".into(),
        kind: ScalarType::Int64,
    }]
}

#[cfg(feature = "mongodb")]
fn mongo(v: &Value) -> oo_connectors::mongodb::MongoEngine {
    use oo_connectors::mongodb::*;
    MongoEngine::new(
        MongoConfig {
            host: "127.0.0.1".into(),
            port: v["port"].as_u64().unwrap() as u16,
            database: "fixture".into(),
            user: "reader".into(),
            auth_database: "admin".into(),
            ca_file: None,
            collections: BTreeMap::from([("events".into(), columns())]),
        },
        zeroize::Zeroizing::new(secret(v, "reader_password")),
        budget(),
    )
    .unwrap()
}

#[cfg(feature = "mongodb")]
#[tokio::test]
#[ignore = "requires a newly created lane-f MongoDB replica-set fixture"]
async fn mongodb_snapshot_changes_and_retention() {
    use mongodb::{
        Client,
        bson::{Bson, doc},
        options::{ClientOptions, Credential, ServerAddress},
    };
    let v = fixture();
    assert_eq!(v["engine"], "mongodb");
    let options = ClientOptions::builder()
        .hosts(vec![ServerAddress::Tcp {
            host: "127.0.0.1".into(),
            port: Some(v["port"].as_u64().unwrap() as u16),
        }])
        .direct_connection(true)
        .credential(
            Credential::builder()
                .username("admin".to_owned())
                .password(secret(&v, "admin_password"))
                .source("admin".to_owned())
                .build(),
        )
        .build();
    let admin = Client::with_options(options).unwrap();
    let db = admin.database("fixture");
    db.run_command(
        doc! { "create": "events", "changeStreamPreAndPostImages": { "enabled": true } },
    )
    .await
    .unwrap();
    let docs: Vec<_> = (0..200).map(|i| doc! { "_id": i, "value": i, "optional": if i % 2 == 0 { Bson::Null } else { Bson::String("".into()) } }).collect();
    db.run_command(
        doc! { "insert": "events", "documents": docs.clone(), "writeConcern": {"w":"majority"} },
    )
    .await
    .unwrap();
    let mut engine = mongo(&v);
    let probe = engine.probe().await.unwrap();
    assert_eq!(probe.version.major, 8);
    let table = TableRef {
        schema: "fixture".into(),
        name: "events".into(),
    };
    let plan = engine.plan_chunks(&table, &budget()).await.unwrap();
    assert!(plan.chunks.len() >= 4);
    let mark = engine.watermark().await.unwrap().unwrap();
    // A separate admin connection writes while the reader consumes the pinned
    // snapshot. No fixture writes are performed by the product reader.
    let writer_db = db.clone();
    let writer = tokio::spawn(async move {
        for i in 0..20 {
            writer_db
                .run_command(doc! { "update": "events",
                "updates": [{"q": {"_id": i}, "u": {"$set": {"value": 1000+i}}}],
                "writeConcern": {"w":"majority"} })
                .await
                .unwrap();
        }
    });
    let mut sink = Sink::default();
    for chunk in &plan.chunks {
        engine
            .read_chunk(chunk, &budget(), &mut sink)
            .await
            .unwrap();
    }
    writer.await.unwrap();
    let expected: BTreeMap<_, _> = docs
        .iter()
        .map(|d| {
            (
                d["_id"].clone().into_canonical_extjson().to_string(),
                Bson::Document(d.clone()).into_canonical_extjson(),
            )
        })
        .collect();
    assert_eq!(
        sink.rows(),
        expected,
        "PIT cut must exclude concurrent changes"
    );
    assert!(engine.stats.rest_us >= engine.stats.busy_us * 3);
    engine.close().await;

    let mut durable = mark;
    let mut rows = sink.rows();
    let cursor_path = PathBuf::from(v["cursor_path"].as_str().unwrap());
    for cycle in 0..3 {
        if cycle == 1 {
            db.run_command(
                doc! { "insert": "events", "documents": [{"_id": 300, "value": 300}],
                "writeConcern": {"w":"majority"} },
            )
            .await
            .unwrap();
            db.run_command(
                doc! { "delete": "events", "deletes": [{"q":{"_id":199},"limit":1}],
                "writeConcern": {"w":"majority"} },
            )
            .await
            .unwrap();
        }
        let mut resumed = mongo(&v);
        let mut changes = 0;
        for _ in 0..10 {
            let Some(batch) = resumed.next_batch(&durable, &budget()).await.unwrap() else {
                break;
            };
            for (change, image) in batch.changes.iter().zip(&batch.images) {
                changes += 1;
                match change {
                    RowChange::Delete { key, .. } => {
                        let KeyValue::Text(id) = &key[0] else {
                            panic!("id");
                        };
                        rows.remove(id);
                    }
                    _ => rows.extend(Sink(vec![image.clone()]).rows()),
                }
            }
            assert!(
                oo_connectors::mongodb::Position::at_or_after(&batch.position, &durable).unwrap()
            );
            durable = batch.position;
            // Models process-restart persistence only, not a target transaction.
            std::fs::write(&cursor_path, serde_json::to_vec(&durable).unwrap()).unwrap();
        }
        if cycle == 2 {
            assert_eq!(changes, 0, "idle C1 poll");
        }
        let result = db
            .run_command(doc! { "find": "events", "batchSize": 1000, "singleBatch": true })
            .await
            .unwrap();
        let expected: BTreeMap<_, _> = result
            .get_document("cursor")
            .unwrap()
            .get_array("firstBatch")
            .unwrap()
            .iter()
            .map(|d| {
                let d = d.as_document().unwrap();
                (
                    d["_id"].clone().into_canonical_extjson().to_string(),
                    Bson::Document(d.clone()).into_canonical_extjson(),
                )
            })
            .collect();
        assert_eq!(rows, expected, "C1 cycle {cycle}");
        resumed.close().await;
        durable = serde_json::from_slice(&std::fs::read(&cursor_path).unwrap()).unwrap();
    }
    for cycle in 0..3 {
        let mut engine = mongo(&v);
        let plan = engine.plan_chunks(&table, &budget()).await.unwrap();
        for chunk in &plan.chunks {
            let digest = engine
                .digest_range(&table, chunk.lo.as_deref(), chunk.hi.as_deref())
                .await
                .unwrap();
            let mut sink = Sink::default();
            let receipt = engine
                .read_chunk(chunk, &budget(), &mut sink)
                .await
                .unwrap();
            assert_eq!(digest.rows, receipt.rows, "C5 cycle {cycle}");
        }
        engine.close().await;
    }
    let mut engine = mongo(&v);
    let mut expired = oo_connectors::mongodb::Position::decode(&durable).unwrap();
    expired.seconds = 1;
    expired.increment = 0;
    expired.resume_token = None;
    let error = engine
        .next_batch(&expired.watermark().unwrap(), &budget())
        .await
        .unwrap_err();
    assert_eq!(error.fact(), Some("mongodb_rebaseline_required_resume"));
    engine.close().await;
    // Cross the server's first-batch wire cap without crossing our chunk cap.
    // Ordinary getMore inherits find's deadline; maxTimeMS on getMore itself
    // is only valid for awaitData cursors.
    db.run_command(doc! { "create": "large" }).await.unwrap();
    let large: Vec<_> = (0..40)
        .map(|i| {
            doc! {
                "_id": i, "value": i, "payload": "a".repeat(500_000)
            }
        })
        .collect();
    for batch in large.chunks(20) {
        db.run_command(doc! { "insert": "large", "documents": batch.to_vec(),
        "writeConcern": {"w":"majority"} })
            .await
            .unwrap();
    }
    let big_budget = ReadBudget {
        max_chunk_bytes: 32 * 1024 * 1024,
        ..budget()
    };
    let mut engine = oo_connectors::mongodb::MongoEngine::new(
        oo_connectors::mongodb::MongoConfig {
            host: "127.0.0.1".into(),
            port: v["port"].as_u64().unwrap() as u16,
            database: "fixture".into(),
            user: "reader".into(),
            auth_database: "admin".into(),
            ca_file: None,
            collections: BTreeMap::from([("large".into(), columns())]),
        },
        zeroize::Zeroizing::new(secret(&v, "reader_password")),
        big_budget.clone(),
    )
    .unwrap();
    let table = TableRef {
        schema: "fixture".into(),
        name: "large".into(),
    };
    let plan = engine.plan_chunks(&table, &big_budget).await.unwrap();
    let mut sink = Sink::default();
    for chunk in &plan.chunks {
        engine
            .read_chunk(chunk, &big_budget, &mut sink)
            .await
            .unwrap();
    }
    let rows = sink.rows();
    assert_eq!(rows.len(), 40);
    assert!(
        rows.values()
            .all(|d| d["payload"].as_str().unwrap().len() == 500_000)
    );
    engine.close().await;
    admin.shutdown().await;
    println!(
        "{}",
        json!({"engine":"mongodb","version":probe.version.raw,
        "source_checks":"snapshot_concurrent_writer,c1_three_cycles,c5_three_reads,retained_floor_rejection,pacing",
        "agent_db_acceptance":false})
    );
}

#[cfg(feature = "opensearch")]
#[tokio::test]
#[ignore = "requires a newly created lane-f authenticated search fixture"]
async fn search_pit_ranges_and_digests() {
    use oo_connectors::opensearch::*;
    let v = fixture();
    let product = match v["engine"].as_str().unwrap() {
        "opensearch" => Product::Opensearch,
        "elasticsearch" => Product::Elasticsearch,
        _ => panic!("wrong fixture"),
    };
    let make_engine = || {
        SearchEngine::new(
            SearchConfig {
                product,
                host: "127.0.0.1".into(),
                port: v["port"].as_u64().unwrap() as u16,
                user: "reader".into(),
                ca_file: v["ca_file"].as_str().map(PathBuf::from),
                schema: "fixture".into(),
                indices: BTreeMap::from([(
                    "events".into(),
                    Index {
                        columns: columns(),
                        range_field: "key".into(),
                        boundaries: vec!["0050".into(), "0100".into(), "0150".into()],
                    },
                )]),
            },
            zeroize::Zeroizing::new(secret(&v, "reader_password")),
            budget(),
        )
        .unwrap()
    };
    let table = TableRef {
        schema: "fixture".into(),
        name: "events".into(),
    };
    let mut previous = None;
    let ca = std::fs::read(v["ca_file"].as_str().unwrap()).unwrap();
    let admin = reqwest::Client::builder()
        .no_proxy()
        .tls_certs_only([reqwest::Certificate::from_pem(&ca).unwrap()])
        .build()
        .unwrap();
    let endpoint = format!("https://127.0.0.1:{}", v["port"].as_u64().unwrap());
    let admin_name = if product == Product::Elasticsearch {
        "elastic"
    } else {
        "admin"
    };
    let denied = admin
        .put(format!("{endpoint}/events/_doc/forbidden"))
        .basic_auth("reader", Some(secret(&v, "reader_password")))
        .header("Content-Type", "application/json")
        .body(r#"{"key":"forbidden","value":0}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(
        denied.status().as_u16(),
        403,
        "source account must not write"
    );
    for cycle in 0..3 {
        let mut engine = make_engine();
        let probe = engine.probe().await.unwrap();
        let plan = engine.plan_chunks(&table, &budget()).await.unwrap();
        assert_eq!(plan.chunks.len(), 4);
        let writer = if cycle == 0 {
            let (admin, endpoint, password) = (
                admin.clone(),
                endpoint.clone(),
                secret(&v, "admin_password"),
            );
            Some(tokio::spawn(async move {
                for i in 0..10 {
                    let r = admin
                        .post(format!("{endpoint}/events/_update/{i}"))
                        .basic_auth(admin_name, Some(&password))
                        .header("Content-Type", "application/json")
                        .body(json!({"doc":{"value":1000+i}}).to_string())
                        .send()
                        .await
                        .unwrap();
                    assert!(r.status().is_success());
                }
                assert!(
                    admin
                        .post(format!("{endpoint}/events/_refresh"))
                        .basic_auth(admin_name, Some(&password))
                        .send()
                        .await
                        .unwrap()
                        .status()
                        .is_success()
                );
            }))
        } else {
            None
        };
        let mut sink = Sink::default();
        let mut digests = Vec::new();
        for chunk in &plan.chunks {
            digests.push(
                engine
                    .digest_range(&table, chunk.lo.as_deref(), chunk.hi.as_deref())
                    .await
                    .unwrap(),
            );
            engine
                .read_chunk(chunk, &budget(), &mut sink)
                .await
                .unwrap();
        }
        let expected: BTreeMap<_,_> = (0..200).map(|i| (i.to_string(),
            json!({"key":format!("{i:04}"),"value":if cycle > 0 && i < 10 { 1000+i } else { i },
                "nested":{"empty":null}}))).collect();
        assert_eq!(sink.rows(), expected);
        if let Some(writer) = writer {
            writer.await.unwrap();
        }
        if let Some(previous) = previous {
            if cycle == 1 {
                assert_ne!(digests, previous, "changed C5 cycle");
            } else {
                assert_eq!(digests, previous, "idle C5 cycle");
            }
        }
        previous = Some(digests);
        assert!(engine.stats.rest_us >= engine.stats.busy_us * 3);
        engine.close().await.unwrap();
        println!(
            "{}",
            json!({"engine":v["engine"],"version":probe.version.raw,
            "source_cycle":cycle,"agent_db_acceptance":false})
        );
    }
}
