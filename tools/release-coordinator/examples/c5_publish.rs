use release_coordinator::{
    Endpoint, FenceRef, TargetConn, install_target, native_publication_context, open_native_target,
    rustls_connector, sha256_hex,
};
use serde_json::{Value, json};
use std::{error::Error, fs, path::Path};
use tokio_postgres::{Client, config::SslMode};

async fn connect(
    database: &str,
    user: &str,
    password: &str,
    ca: &[u8],
) -> Result<Client, Box<dyn Error>> {
    let mut config = tokio_postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(25479)
        .dbname(database)
        .user(user)
        .password(password.trim())
        .ssl_mode(SslMode::Require);
    let (client, connection) = config
        .connect(rustls_connector(ca).map_err(|e| e.to_string())?)
        .await?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let database = args.get(1).ok_or("database")?;
    let source = args.get(2).ok_or("source")?;
    let out = Path::new(args.get(3).ok_or("output directory")?).join(format!(
        "publication-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs::create_dir(&out)?;
    let root = Path::new("/tmp/oo-wf-c5e2e-m1");
    let password = fs::read_to_string(root.join("run/admin"))?;
    let reader_password = fs::read_to_string(root.join("run/worker"))?;
    let ca = fs::read(root.join("tls/ca.crt"))?;
    let admin = connect(database, "postgres", &password, &ca).await?;
    let row = admin.query_one("SELECT r.run_id::text,r.source_generation,r.target_generation,r.config_digest,t.schema_ident,t.table_ident,t.physical_name,t.applied_rows,t.canonical_digest,w.consistency_grade,w.position_lineage FROM _oo.table_freshness f JOIN _oo.full_import_table t ON t.physical_name=f.physical_name JOIN _oo.full_import_run r USING(run_id) JOIN _oo.source_watermark w ON w.source=f.source WHERE f.source=$1 AND f.source_generation=r.source_generation", &[source]).await?;
    let run: String = row.get(0);
    let generation: i64 = row.get(1);
    let target_generation: i64 = row.get(2);
    let config_digest: String = row.get(3);
    let schema: String = row.get(4);
    let table: String = row.get(5);
    let physical: String = row.get(6);
    let accepted: i64 = row.get(7);
    let canonical: String = row.get(8);
    let grade: String = row.get(9);
    let lineage: String = row.get(10);
    let ack: bool = admin.query_one("SELECT EXISTS(SELECT FROM jw_native.watermarks w JOIN jw_native.bindings b ON b.id=w.binding_id JOIN jobs.tasks j ON j.id=w.task_id WHERE b.source_id=$1 AND w.source_generation=$2 AND j.state='succeeded')", &[source,&generation]).await?.get(0);
    assert!(ack, "worker has not acknowledged this source generation");
    let columns = admin.query("SELECT logical_column,physical_name FROM _oo.physical_mappings WHERE source=$1 AND run_generation=$2 AND config_digest=$3 AND level='c' AND logical_namespace=$4 AND logical_table=$5 ORDER BY logical_column", &[source,&generation,&config_digest,&schema,&table]).await?;
    let logical: Vec<String> = columns.iter().map(|r| r.get(0)).collect();
    let mapped: Vec<String> = columns.iter().map(|r| r.get(1)).collect();
    let rejects: i64 = admin.query_one("SELECT COALESCE(sum(rejected_rows),0)::bigint FROM _oo.full_import_quality WHERE run_id=$1::text::uuid AND kind='batch'", &[&run]).await?.get(0);
    assert_eq!(rejects, 0);
    let receipt = json!({"protocol":"oo.pipeline.native-publication-receipt/v1","collection_mode":"per_table","source_id":source,"source_generation":generation,"target_generation":target_generation,"position_codec":"snapshot-zero/v1","position_lineage":lineage,"applied_pos":"0","consistency":"per_table","consistency_grade":grade,"cross_table_consistent":false,"run_id":run,"run_ids":[run],"tables":[{"schema":schema,"table":table,"physical_name":physical,"accepted_rows":accepted,"canonical_digest":canonical,"rejects":rejects,"run_id":run,"source_generation":generation,"target_generation":target_generation,"applied_pos":"0","position_lineage":lineage,"capture_method":"c5_diff","delete_detection":"reconcile","consistency_grade":grade}]});
    let context = json!({"publication":{"protocol":"oo.pipeline.native-publication-receipt/v1","run_id":run,"receipt_sha256":sha256_hex(&serde_json::to_vec(&receipt)?),"receipt":receipt}});
    let publication = native_publication_context(&context)
        .map_err(|e| e.to_string())?
        .ok_or("publication context")?;
    let dataset = format!("{schema}.{table}");
    let effective = json!({"protocol":"oo.builder.effective-oos/v1","project":"c5-e2e","ontology_version":"c5-v1","catalog_version":"c5-v1","policy":{"id":"c5-policy","version":"1"},"knowledge":{"id":"c5-knowledge","version":"1"},"ontology":{"spec_version":"oos/v1","name":"fixture","locale":"en-US","include_schemas":[schema],"default_tier_preference":["file"],"publish":{"include_unreviewed":[]},"classification":{"auto_accept":"rule_high_confidence"},"lifecycle":{"deprecation_min_releases":2},"compile":{"per_role":true,"budgets":{"readme_tokens":2000,"table_file_tokens":3000},"sample_values":{"default":"masked","max_per_column":0}}},"datasets":[{"schema_version":"oos/v1","dataset":dataset,"label":"Anonymous fixture","tier":"file","grain":"one row per key","primary_key":["id"],"columns":logical.iter().map(|name|json!({"name":name,"pii":"none","classification":{"state":"confirmed"}})).collect::<Vec<_>>()}]});
    let visibility = json!({"protocol":"oo.builder.role-visibility/v1","project":"c5-e2e","principal":"fixture","role":"oo_r_c5","ontology_version":"c5-v1","catalog_version":"c5-v1","policy":{"id":"c5-policy","version":"1"},"knowledge":{"id":"c5-knowledge","version":"1"},"issued_for_compiler":"oo-builder/role-pack/v1","datasets":[{"dataset":dataset,"row_security":"rls","global_stats":"denied","columns":logical}],"limits":{"max_file_bytes":262144,"max_total_bytes":1048576,"max_entries":128}});
    let pack = oo_builder::compile_role_pack(
        &serde_json::to_string(&effective)?,
        &serde_json::to_string(&visibility)?,
    )?;
    fs::write(
        out.join("publication-input.json"),
        serde_json::to_vec_pretty(&context)?,
    )?;
    fs::write(out.join("fixture-pack.json"), &pack.artifact)?;
    let endpoint = Endpoint {
        pack_ceilings: None,
        host: "127.0.0.1".into(),
        port: 25479,
        database: database.clone(),
    };
    let target = TargetConn {
        endpoint: &endpoint,
        user: "oo_target_admin",
        password: password.trim(),
        ca_pem: &ca,
    };
    let fence = FenceRef {
        schema_name: "_oo",
        relation_name: &physical,
        role_name: "oo_r_c5",
    };
    admin.batch_execute("GRANT USAGE ON SCHEMA _oo TO oo_r_c5; GRANT EXECUTE ON FUNCTION _oo.publish_native_per_table_collection(uuid,text,jsonb) TO oo_target_admin").await?;
    let installed = install_target(&target, &fence)
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(installed.code, 0);
    let opened = open_native_target(
        &target,
        &fence,
        &mapped,
        &pack.pack_digest,
        "c5-v1",
        "c5-native",
        &publication,
    )
    .await
    .map_err(|e| e.to_string())?;
    assert_eq!(opened.code, 0);
    fs::write(
        out.join("publication-output.json"),
        serde_json::to_vec_pretty(&opened.body)?,
    )?;
    let replay = open_native_target(
        &target,
        &fence,
        &mapped,
        &pack.pack_digest,
        "c5-v1",
        "c5-native",
        &publication,
    )
    .await
    .map_err(|e| e.to_string())?;
    assert_eq!(replay.code, 0);
    let reader = connect(database, "c5_reader", &reader_password, &ca).await?;
    let read_only: String = reader
        .query_one("SHOW transaction_read_only", &[])
        .await?
        .get(0);
    assert_eq!(read_only, "on");
    let pipeline: bool = reader
        .query_one(
            "SELECT pg_has_role(current_user,'oo_pipeline_runtime','MEMBER')",
            &[],
        )
        .await?
        .get(0);
    assert!(!pipeline);
    let count: i64 = reader
        .query_one(
            &format!(
                "SELECT count(\"{}\") FROM _oo.\"{physical}\"",
                mapped.first().ok_or("columns")?
            ),
            &[],
        )
        .await?
        .get(0);
    assert_eq!(count, accepted);
    let state: String = admin
        .query_one(
            "SELECT state FROM _oo.full_import_run WHERE run_id=$1::text::uuid",
            &[&run],
        )
        .await?
        .get(0);
    assert_eq!(state, "published");
    let result: Value = json!({"source":source,"source_generation":generation,"run_id":run,"publication":"published","fresh_session_reader_rows":count,"reader_read_only":true,"reader_has_pipeline_role":false,"publication_replay":true,"pack_digest":pack.pack_digest,"gateway":"not_run","evidence_directory":out.to_string_lossy()});
    fs::write(
        out.join("publication-result.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{result}");
    Ok(())
}
