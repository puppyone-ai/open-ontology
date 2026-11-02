use mysql_async::prelude::Queryable;
use mysql_preview::c5_layered::{connect, produce};
use oo_connectors::mysql::chunk;
use oo_pipeline::target_installer::sha256_hex;
use serde_json::json;

#[tokio::test]
async fn owned_full_baseline_unbounded_range_is_read_completely() {
    let root = std::path::Path::new("/tmp/oo-wf-c5e2e-m1");
    let config = json!({"source_id":"m1_fixture","schema":"sample","table":"items","secret_ref":"secret://m1/reader","connection":{"host":"127.0.0.1","port":23361,"user":"reader","secret_file":root.join("reader"),"tls":"disabled","plaintext_authorized":true},"audit":{"mode":"grant_enforced"},"max_rows":1000,"max_bytes":1048576,"target":{}});
    let mut conn = connect(&config).await.unwrap();
    let columns: Vec<(String,String,Option<String>,String)> = conn.query("SELECT COLUMN_NAME,COLUMN_TYPE,COLLATION_NAME,IS_NULLABLE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA='sample' AND TABLE_NAME='items' ORDER BY ORDINAL_POSITION").await.unwrap();
    let keys = chunk::key_columns(&mut conn, "sample", "items")
        .await
        .unwrap();
    let fingerprint =
        sha256_hex(&serde_json::to_vec(&json!({"columns":columns,"keys":keys})).unwrap());
    let authority = json!({"source_id":"m1_fixture","schema":"sample","table":"items","schema_fingerprint":fingerprint,"source_generation":1,"chunk_topology_revision":1,"next_chunk_index":0,"chunks":[{"chunk_id":"0","lo":null,"hi":null,"membership_revision":1,"row_count":3}]});
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let input = json!({"protocol":"oo.jobs.native-producer/v1","source_id":"m1_fixture","binding":"00000000-0000-4000-8000-000000000001","pin":2,"method":"C5","c5_mode":"layered","acknowledged_source_generation":1,"c5_due":[true,false,true,false],"c5_observed_at_ms":now,"c5_layered":{"policy":{"duty_per_mille":1000,"threads_running_max":32,"execution_window_utc":[0,1440]},"recent_column":null,"window_ms":60000,"overlap_ms":60000,"server_timeout_ms":5000,"client_timeout_ms":10000,"execution_timezone":"UTC"}});
    let out = root.join(format!("unbounded-test-{}", std::process::id()));
    std::fs::create_dir(&out).unwrap();
    let result = produce(conn, &config, &input, &authority, &out)
        .await
        .unwrap();
    assert_eq!(result["protocol"], "oo.pipeline.native-c5-range/v1");
    assert!(result["hi"].is_null());
    assert_eq!(result["row_count"], 3);
    assert_eq!(result["members"].as_array().unwrap().len(), 3);
    let capturer: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("capturer.json")).unwrap()).unwrap();
    assert_eq!(capturer["source_audit"]["source"], "grant_enforced");
    for field in [
        "protocol",
        "source_id",
        "schema_fingerprint",
        "members",
        "arrow_sha256",
        "row_count",
    ] {
        assert_eq!(capturer["range_scope"][field], result[field]);
    }
}
