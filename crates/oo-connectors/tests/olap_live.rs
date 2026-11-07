//! Live T0 and engine checks for ClickHouse, Doris and StarRocks.
//!
//! Ignored by default. `testing/engines/olap/run.py` writes the owner-private
//! fixture config named by `OO_OLAP_FIXTURE` and runs this against a fresh,
//! lane-labelled container. Passing unit tests are not evidence that this ran.
#![cfg(feature = "olap")]

use std::collections::BTreeMap;

use arrow_array::RecordBatch;
use oo_connectors::SourceError;
use oo_connectors::engine::{
    Capability, CapabilityState, ChunkSink, EngineFamily, ReadBudget, SourceEngine, TableRef,
};
use oo_connectors::olap::{OlapConfig, OlapEngine, OlapTls};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    family: String,
    host: String,
    port: u16,
    database: String,
    user: String,
    password_file: String,
    ca_file: Option<String>,
    expect_version: String,
    /// Table name to admin-observed row count.
    counts: BTreeMap<String, u64>,
    /// Tables expected to be partition-aligned, with their partition count.
    #[serde(default)]
    partitions: BTreeMap<String, usize>,
    chunk_rows: u64,
    /// Capability expected to need setup before the fixture grants it.
    #[serde(default)]
    needs_setup: Option<String>,
    out: String,
}

#[derive(Default)]
struct Collect(Vec<RecordBatch>);
impl ChunkSink for Collect {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        self.0.push(batch);
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires OO_OLAP_FIXTURE from testing/engines/olap/run.py"]
async fn olap_t0_snapshot_digest_split() {
    let path = std::env::var("OO_OLAP_FIXTURE").expect("OO_OLAP_FIXTURE");
    let fx: Fixture = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let family = match fx.family.as_str() {
        "clickhouse" => EngineFamily::Clickhouse,
        "doris" => EngineFamily::Doris,
        "starrocks" => EngineFamily::Starrocks,
        other => panic!("family {other}"),
    };
    let password = zeroize::Zeroizing::new(
        std::fs::read_to_string(&fx.password_file)
            .unwrap()
            .trim()
            .to_string(),
    );
    let budget = ReadBudget {
        max_chunk_rows: fx.chunk_rows,
        ..ReadBudget::default()
    };
    let config = OlapConfig {
        host: fx.host.clone(),
        port: fx.port,
        database: fx.database.clone(),
        user: fx.user.clone(),
        tls: fx.ca_file.as_ref().map(|ca| OlapTls {
            ca_file: ca.into(),
            server_name: None,
        }),
        connect_timeout_ms: 10_000,
    };
    let mut engine = OlapEngine::new(family, config, password, budget.clone()).unwrap();
    let mut out = serde_json::Map::new();

    // T0: identity, capability grades and DBA statements.
    let report = engine.probe().await.unwrap();
    assert_eq!(
        report.version.to_string(),
        fx.expect_version,
        "version identity"
    );
    if let Some(cap) = &fx.needs_setup {
        let state = report
            .capabilities
            .iter()
            .find(|(c, _)| format!("{c:?}") == *cap)
            .map(|(_, s)| s)
            .expect("capability");
        assert!(
            matches!(state, CapabilityState::NeedsSetup { requirements } if requirements.iter().all(|r| r.dba_statement.is_some())),
            "{cap} must list a DBA statement"
        );
        // Negative T0: the grade and DBA statement are the deliverable.
        out.insert("probe".into(), serde_json::to_value(&report).unwrap());
        engine.close().await;
        std::fs::write(&fx.out, serde_json::to_vec_pretty(&out).unwrap()).unwrap();
        return;
    } else {
        for cap in [
            Capability::Catalog,
            Capability::ChunkedSnapshot,
            Capability::DigestDiff,
            Capability::PartitionRefresh,
        ] {
            assert_eq!(
                report.capabilities[&cap],
                CapabilityState::Available,
                "{cap:?}"
            );
        }
    }
    assert!(matches!(
        report.capabilities[&Capability::LogCdc],
        CapabilityState::Unsupported { .. }
    ));
    out.insert("probe".into(), serde_json::to_value(&report).unwrap());
    let discovered = engine.discover_tables(&budget).await.unwrap();
    let mut tables = Vec::new();
    for (name, expected) in &fx.counts {
        let table = TableRef {
            schema: fx.database.clone(),
            name: name.clone(),
        };
        assert!(discovered.contains(&table), "discovered {name}");
        let plan = engine.plan_chunks(&table, &budget).await.unwrap();
        assert!(plan.chunks.first().unwrap().lo.is_none());
        assert!(plan.chunks.last().unwrap().hi.is_none());
        for w in plan.chunks.windows(2) {
            assert_eq!(w[0].hi, w[1].lo, "{name} adjacency");
        }
        if let Some(parts) = fx.partitions.get(name) {
            let starts = plan
                .chunks
                .iter()
                .filter(|c| c.lo.as_ref().is_some_and(|l| l.len() == 1))
                .count();
            // Sub-splits of a large partition may add more one-column bounds.
            assert!(starts + 1 >= *parts, "{name} partition-aligned ranges");
        }
        let mut rows = 0;
        let mut largest = None::<(u64, usize)>;
        for (i, chunk) in plan.chunks.iter().enumerate() {
            let mut sink = Collect::default();
            let receipt = engine.read_chunk(chunk, &budget, &mut sink).await.unwrap();
            assert_eq!(sink.0.len(), 1, "schema-bearing batch");
            assert!(receipt.rows <= budget.max_chunk_rows);
            let digest = engine
                .digest_range(&table, chunk.lo.as_deref(), chunk.hi.as_deref())
                .await
                .unwrap();
            assert_eq!(digest.rows, receipt.rows, "{name} chunk {i} digest rows");
            rows += receipt.rows;
            if largest.is_none_or(|(n, _)| receipt.rows > n) {
                largest = Some((receipt.rows, i));
            }
        }
        assert_eq!(rows, *expected, "{name} rows");
        let whole = engine.digest_range(&table, None, None).await.unwrap();
        assert_eq!(whole.rows, *expected);
        let again = engine.digest_range(&table, None, None).await.unwrap();
        assert_eq!(whole, again, "{name} digest is deterministic");
        let (parent_rows, index) = largest.unwrap();
        let parent = &plan.chunks[index];
        let split = engine.split_chunk(parent, &budget).await.unwrap();
        if let Some(children) = &split {
            assert_eq!(children.first().unwrap().lo, parent.lo);
            assert_eq!(children.last().unwrap().hi, parent.hi);
            let mut sum = 0;
            for child in children {
                assert!(child.plan_revision > parent.plan_revision);
                let mut sink = Collect::default();
                sum += engine
                    .read_chunk(child, &budget, &mut sink)
                    .await
                    .unwrap()
                    .rows;
            }
            assert_eq!(sum, parent_rows, "{name} split coverage");
        } else {
            assert!(
                parent_rows < 2,
                "{name} split refused with {parent_rows} rows"
            );
        }
        let epoch = engine
            .source_epoch(std::slice::from_ref(&table))
            .await
            .unwrap();
        assert_eq!(
            epoch,
            engine
                .source_epoch(std::slice::from_ref(&table))
                .await
                .unwrap()
        );
        tables.push(serde_json::json!({
            "table": name, "rows": rows, "chunks": plan.chunks.len(), "strategy": plan.strategy,
            "key": plan.key, "split_children": split.map(|c| c.len()), "digest": whole.digest,
            "excluded_columns": engine.excluded_columns(&table),
        }));
    }
    out.insert("tables".into(), tables.into());
    out.insert(
        "watermark".into(),
        serde_json::to_value(engine.watermark().await.unwrap()).unwrap(),
    );
    out.insert(
        "stats".into(),
        serde_json::to_value(engine.stats()).unwrap(),
    );
    out.insert("budget".into(), serde_json::to_value(&budget).unwrap());
    engine.close().await;
    std::fs::write(&fx.out, serde_json::to_vec_pretty(&out).unwrap()).unwrap();
}
