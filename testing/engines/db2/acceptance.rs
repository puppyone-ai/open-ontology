use super::*;

#[derive(Default)]
struct Sink(Vec<RecordBatch>);
impl ChunkSink for Sink {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        self.0.push(batch);
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires the lane-owned Db2 fixture and worker"]
async fn real_db2_keysets_digest_budget_and_empty_schema() {
    let directory = std::path::PathBuf::from(std::env::var("OO_DB2_EVIDENCE").unwrap());
    let password = std::fs::read_to_string(directory.join(".private/reader")).unwrap();
    let config = ConnectConfig::new(
        "127.0.0.1",
        50000,
        "FIXTURE",
        "ooreader",
        Zeroizing::new(password),
    );
    let mut engine =
        Db2Engine::connect(&mut Command::new(directory.join("worker")), &config).unwrap();
    let report = engine.probe().await.unwrap();
    assert_eq!(
        (
            report.version.major,
            report.version.minor,
            report.version.patch
        ),
        (11, 5, 9)
    );
    let b = ReadBudget {
        max_chunk_rows: 100,
        ..ReadBudget::default()
    };
    let mut counts = BTreeMap::new();
    for (name, expected) in [
        ("EVENTS", 2000),
        ("COMPOSITE", 5),
        ("EMPTY", 0),
        ("NO_KEY", 3),
    ] {
        let table = TableRef {
            schema: "FIXTURE".into(),
            name: name.into(),
        };
        let plan = engine.plan_chunks(&table, &b).await.unwrap();
        let mut count = 0;
        for chunk in &plan.chunks {
            let digest = engine
                .digest_range(&table, chunk.lo.as_deref(), chunk.hi.as_deref())
                .await
                .unwrap();
            let mut sink = Sink::default();
            let receipt = engine.read_chunk(chunk, &b, &mut sink).await.unwrap();
            assert_eq!(receipt.rows, digest.rows);
            assert_eq!(sink.0.len(), 1);
            assert!(!sink.0[0].schema().fields().is_empty());
            count += receipt.rows;
            let second = engine
                .digest_range(&table, chunk.lo.as_deref(), chunk.hi.as_deref())
                .await
                .unwrap();
            assert_eq!(digest, second);
        }
        assert_eq!(count, expected);
        counts.insert(name, count);
        let mut forged = plan.chunks[0].clone();
        forged.plan_revision += 1;
        let mut sink = Sink::default();
        assert!(engine.read_chunk(&forged, &b, &mut sink).await.is_err());
        assert!(sink.0.is_empty());
        let tiny = ReadBudget {
            max_chunk_bytes: 1,
            ..b.clone()
        };
        assert!(
            engine
                .read_chunk(&plan.chunks[0], &tiny, &mut sink)
                .await
                .is_err()
        );
        assert!(sink.0.is_empty());
    }
    assert_eq!(engine.stats().sessions, 1);
    assert!(engine.stats().sleep_micros >= engine.stats().read_micros * 3);
    std::fs::write(
        directory.join("engine-acceptance.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "probe": report, "rows": counts, "resources": engine.stats(),
            "keyset_digest_empty_budget": "pass",
            "scope": "four synthetic tables, not the complete shared matrix"
        }))
        .unwrap(),
    )
    .unwrap();
}

#[tokio::test]
#[ignore = "requires the lane-owned full Db2 fixture and private grant probe"]
async fn real_db2_missing_grant_and_c5_change() {
    let directory = std::path::PathBuf::from(std::env::var("OO_DB2_EVIDENCE").unwrap());
    let password = std::fs::read_to_string(directory.join(".private/reader")).unwrap();
    let mut config = ConnectConfig::new(
        "127.0.0.1",
        50000,
        "FIXTURE",
        "ooreader",
        Zeroizing::new(password),
    );
    let table = TableRef {
        schema: "FIXTURE".into(),
        name: "EVENTS".into(),
    };
    config.scope = vec![
        table.clone(),
        TableRef {
            schema: "FIXTURE".into(),
            name: "PRIVATE_PROBE".into(),
        },
    ];
    let mut engine =
        Db2Engine::connect(&mut Command::new(directory.join("worker")), &config).unwrap();
    let probe = engine.probe().await.unwrap();
    let requirements = probe.requirements();
    assert_eq!(requirements.len(), 1);
    assert_eq!(
        requirements[0].dba_statement.as_deref(),
        Some("GRANT SELECT ON \"FIXTURE\".\"PRIVATE_PROBE\" TO USER \"OOREADER\"")
    );
    let b = ReadBudget::default();
    let plan = engine.plan_chunks(&table, &b).await.unwrap();
    let chunk = &plan.chunks[0];
    let before = engine
        .digest_range(&table, chunk.lo.as_deref(), chunk.hi.as_deref())
        .await
        .unwrap();
    let owned: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("fixture-owned.json")).unwrap())
            .unwrap();
    let container = owned["container"].as_str().unwrap();
    assert!(container.starts_with("oo-e-db2-e-s5f-"));
    let admin = |value: &str| {
        use std::io::Write;
        let mut child = Command::new("docker")
            .args([
                "exec",
                "-i",
                container,
                "su",
                "-",
                "db2inst1",
                "-c",
                "db2 +p -st",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                format!(
                    "CONNECT TO FIXTURE;\nUPDATE FIXTURE.EVENTS SET VALUE='{value}' WHERE ID=2;\n"
                )
                .as_bytes(),
            )
            .unwrap();
        assert!(child.wait().unwrap().success());
    };
    admin("digest-change");
    let changed = engine
        .digest_range(&table, chunk.lo.as_deref(), chunk.hi.as_deref())
        .await;
    admin("row-2");
    let changed = changed.unwrap();
    assert_eq!(before.rows, changed.rows);
    assert_ne!(before.digest, changed.digest);
    let restored = engine
        .digest_range(&table, chunk.lo.as_deref(), chunk.hi.as_deref())
        .await
        .unwrap();
    assert_eq!(before, restored);
    let mut sink = Sink::default();
    let tiny = ReadBudget {
        max_chunk_rows: 1,
        ..b.clone()
    };
    assert!(engine.read_chunk(chunk, &tiny, &mut sink).await.is_err());
    assert!(sink.0.is_empty());
    assert!(engine.probe().await.is_ok());
    std::fs::write(
        directory.join("t0-c5-acceptance.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "probe": probe, "missing_select_grant": "pass", "c5_changed_and_restored": "pass",
            "before": before, "changed": changed, "resources": engine.stats(),
        }))
        .unwrap(),
    )
    .unwrap();
}
