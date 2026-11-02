use serde_json::Value;

#[tokio::test]
#[ignore = "live fixture: requires isolated owned PostgreSQL and module fixture configuration; run with --ignored"]
async fn library_query_gate_rejects_closed_and_stale_heads() {
    let fixture: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("OO_LIBRARY_FIXTURE").expect("owned fixture")).unwrap(),
    )
    .unwrap();
    let port: u16 = std::env::var("OO_LIBRARY_PG_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let (mut client, connection) = tokio_postgres::connect(
        &format!(
            "host=127.0.0.1 port={port} user=postgres dbname={}",
            fixture["database"].as_str().unwrap()
        ),
        tokio_postgres::NoTls,
    )
    .await
    .unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let sources = vec!["lib_s0".into(), "lib_s1".into(), "lib_s2".into()];
    let tx = client.transaction().await.unwrap();
    tx.batch_execute("UPDATE _oo.native_library_head SET state='closed'")
        .await
        .unwrap();
    assert!(
        !oo_gateway::query_executor::verify_library_sources(&tx, &sources)
            .await
            .unwrap()
    );
    tx.batch_execute("UPDATE _oo.native_library_head SET state='pending'")
        .await
        .unwrap();
    assert!(
        !oo_gateway::query_executor::verify_library_sources(&tx, &sources)
            .await
            .unwrap()
    );
    tx.batch_execute("UPDATE _oo.native_library_head SET state='ready'")
        .await
        .unwrap();
    assert!(
        oo_gateway::query_executor::verify_library_sources(&tx, &sources)
            .await
            .unwrap()
    );
    tx.batch_execute("UPDATE _oo.source_watermark SET source_generation=source_generation+1 WHERE source='lib_s2'").await.unwrap();
    assert!(
        !oo_gateway::query_executor::verify_library_sources(&tx, &sources)
            .await
            .unwrap(),
        "one stale shard closes the entire library"
    );
    assert!(
        !oo_gateway::query_executor::verify_library_sources(&tx, &["lib_s0".into()])
            .await
            .unwrap(),
        "a query against another member must also fail closed"
    );
    assert!(
        oo_gateway::query_executor::verify_library_sources(&tx, &["unadopted_source".into()])
            .await
            .unwrap()
    );
    tx.rollback().await.unwrap();
}
