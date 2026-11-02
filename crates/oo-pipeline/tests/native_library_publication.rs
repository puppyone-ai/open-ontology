use oo_pipeline::physical_mapping::{Level, MappingRequest, plan_physical_mapping};
use oo_pipeline::target_installer as ti;
use serde_json::{Value, json};
use sqlx::{Connection, Executor, PgConnection};

async fn connect(database: &str) -> PgConnection {
    let port = std::env::var("OO_LIBRARY_PG_PORT").expect("owned OO_LIBRARY_PG_PORT");
    PgConnection::connect(&format!("postgres://postgres@127.0.0.1:{port}/{database}"))
        .await
        .unwrap()
}
async fn install(db: &mut PgConnection) {
    install_until(db, true).await;
}

async fn install_until(db: &mut PgConnection, latest: bool) {
    ti::install(db).await.unwrap();
    db.execute("CREATE SCHEMA jobs; CREATE TABLE jobs.tasks(id uuid PRIMARY KEY,state text,generation bigint,lease_until timestamptz,project_id uuid); DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_jobs_owner') THEN CREATE ROLE oo_jobs_owner NOLOGIN; END IF; IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_jobs_runtime') THEN CREATE ROLE oo_jobs_runtime NOLOGIN; END IF; END $$; GRANT USAGE ON SCHEMA jobs TO oo_target_owner,oo_pipeline_runtime; GRANT SELECT ON jobs.tasks TO oo_target_owner").await.unwrap();
    ti::install_full_import(db).await.expect("0005");
    ti::install_catalog_before_empty(db)
        .await
        .expect("0006-0008");
    ti::install_jsonb_document(db).await.expect("0009");
    ti::install_native_update(db).await.expect("0010");
    ti::install_native_partition(db).await.expect("0011");
    ti::install_native_partition_rebind(db).await.expect("0012");
    ti::install_native_interval(db).await.expect("0013");
    ti::install_native_olap_partition(db)
        .await
        .expect("0014-0016");
    ti::install_native_pg_transaction_group(db)
        .await
        .expect("0017");
    ti::install_native_empty_catalog(db).await.expect("0018");
    ti::install_native_pg_group_receipt(db).await.expect("0019");
    ti::install_native_pg_group_prior(db).await.expect("0020");
    ti::install_native_pg_group_generation(db)
        .await
        .expect("0021");
    ti::install_native_pg_single_transaction(db)
        .await
        .expect("0022");
    ti::install_native_sqlserver_ct(db).await.expect("0023");
    ti::install_native_gtid_transaction(db).await.expect("0024");
    ti::install_native_c1_source_epoch(db).await.expect("0025");
    ti::install_native_c1_source_epoch_runtime(db)
        .await
        .expect("0026");
    ti::install_native_c1_baseline_stage(db)
        .await
        .expect("0028");
    ti::install_native_c1_gtid_stage_identity(db)
        .await
        .expect("0029");
    ti::install_native_c1_gtid_measured_identity(db)
        .await
        .expect("0030");
    ti::install_native_c1_baseline_typed_bridge(db)
        .await
        .expect("0031");
    ti::install_native_c1_baseline_lease_rebind(db)
        .await
        .expect("0032");
    ti::install_native_c1_baseline_date_quality(db)
        .await
        .expect("0033");
    ti::install_native_c1_baseline_key_join(db)
        .await
        .expect("0034");
    ti::install_native_c1_ack_seed(db).await.expect("0035");
    ti::install_native_publication(db).await.expect("0027");
    ti::install_native_c1_collection_publication(db)
        .await
        .expect("0036");
    ti::install_native_c1_timed_receipt(db).await.expect("0037");
    ti::install_native_c1_quiet_baseline_time(db)
        .await
        .expect("0038");
    ti::install_native_per_table_publication(db)
        .await
        .expect("0039");
    ti::install_native_source_progress_read(db)
        .await
        .expect("0040");
    ti::install_native_partition_observation_read(db)
        .await
        .expect("0041");
    ti::install_native_c1_chunked_baseline(db)
        .await
        .expect("0042");
    ti::install_native_c1_chunked_baseline_final(db)
        .await
        .expect("0043");
    ti::install_native_source_instance_grant(db).await.unwrap();
    ti::install_native_c1_ack_seed_format(db).await.unwrap();
    ti::install_native_c5_range(db).await.unwrap();
    ti::install_native_c5_range_apply(db).await.unwrap();
    ti::install_native_c5_scan(db).await.unwrap();
    ti::install_native_c5_replay_guard(db).await.unwrap();
    ti::install_native_c5_full_initialize(db).await.unwrap();
    ti::install_native_c5_closed_observation(db).await.unwrap();
    let prior: String = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(m) ORDER BY version,phase)::text FROM _oo.schema_migrations m",
    )
    .fetch_one(&mut *db)
    .await
    .unwrap();
    ti::install_native_c5_multipart(db).await.unwrap();
    ti::install_masked_rejects(db).await.unwrap();
    ti::install_multipart_cells(db).await.unwrap();
    ti::install_native_c5_publication(db).await.unwrap();
    ti::install_native_c5_single_baseline(db).await.unwrap();
    ti::install_native_c5_multipart_baseline_guard(db)
        .await
        .unwrap();
    ti::install_native_c5_wide_columns(db).await.unwrap();
    ti::install_native_c5_cells_array(db).await.unwrap();
    ti::install_native_library_publication(db).await.unwrap();
    ti::install_native_library_closed_pointer(db).await.unwrap();
    ti::install_native_library_closed_receipt(db).await.unwrap();
    if latest {
        ti::install_native_library_existing_receipt_fields(db)
            .await
            .unwrap();
    }
    let preserved: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM jsonb_array_elements($1::jsonb) old WHERE NOT EXISTS(SELECT 1 FROM _oo.schema_migrations m WHERE to_jsonb(m)=old))").bind(prior).fetch_one(&mut *db).await.unwrap();
    assert!(preserved, "0052 through 0061 preserves every old marker");
}
async fn mapping(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    source: &str,
    table: &str,
    generation: i64,
    level: Level,
    column: &str,
) -> String {
    let request = MappingRequest {
        source: source.into(),
        config_digest: "a".repeat(64),
        run_generation: generation,
        level,
        logical_namespace: "app".into(),
        logical_table: if level == Level::Namespace {
            "".into()
        } else {
            table.into()
        },
        logical_column: column.into(),
    };
    let plan = plan_physical_mapping(&request).unwrap();
    let _: String =
        sqlx::query_scalar("SELECT _oo.register_physical_mapping($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(source)
            .bind(&request.config_digest)
            .bind(generation)
            .bind(match level {
                Level::Namespace => "namespace",
                Level::Table => "table",
                Level::Column => "column",
            })
            .bind("app")
            .bind(&request.logical_table)
            .bind(column)
            .bind(plan.identity_digest.to_vec())
            .fetch_one(&mut **tx)
            .await
            .unwrap();
    plan.physical_name
}

async fn load_inner(
    db: &mut PgConnection,
    source: &str,
    table: &str,
    (generation, revision): (i64, i64),
    rows: &[(&str, &str)],
    (text_key, fail, is_range): (bool, bool, bool),
) -> (Value, String) {
    use oo_pipeline::native_update::{
        FixtureCell, FixtureColumn, FixtureKind, encode_empty_fixture_arrow, encode_fixture_arrow,
    };
    let columns = [
        FixtureColumn {
            name: "id".into(),
            nullable: false,
            kind: if text_key {
                FixtureKind::Utf8
            } else {
                FixtureKind::Int64
            },
        },
        FixtureColumn {
            name: "value".into(),
            nullable: false,
            kind: FixtureKind::Utf8,
        },
    ];
    let values = rows
        .iter()
        .map(|(k, v)| {
            vec![
                if text_key {
                    FixtureCell::Text(k.to_string())
                } else {
                    FixtureCell::Int(k.parse().unwrap())
                },
                FixtureCell::Text(v.to_string()),
            ]
        })
        .collect::<Vec<_>>();
    let bytes = if values.is_empty() {
        encode_empty_fixture_arrow(&columns)
    } else {
        encode_fixture_arrow(&columns, &values)
    }
    .unwrap();
    let arrow_digest = oo_pipeline::native_arrow::arrow_sha256(&bytes);
    let scope = json!({"protocol":"oo.pipeline.native-c5-range/v1","source_engine":"mysql","capture_method":"c5_diff","source_id":source,"schema":"app","table":table,"schema_fingerprint":"a".repeat(64),"chunk_topology_revision":1,"chunk_id":"0","lo":null,"hi":null,"prior_membership_revision":revision,"parent_generation":generation-1,"source_generation":generation,"arrow_sha256":arrow_digest,"row_count":rows.len(),"delete_detection":"reconcile","position_codec":"snapshot-zero/v1","applied_pos":"0","members":rows.iter().map(|(k,_)|if text_key{json!([{"kind":"text","value":k.as_bytes().iter().map(|b|format!("{b:02x}")).collect::<String>()}])}else{json!([{"kind":"int","value":k.parse::<i64>().unwrap()}])}).collect::<Vec<_>>()});
    let parsed = oo_pipeline::native_c5_range::RangeScope::parse(&scope).unwrap();
    parsed.validate_arrow_digest(&bytes).unwrap();
    parsed
        .validate_decoded_keys(
            &oo_pipeline::native_arrow::decode_arrow_stream(&bytes).unwrap(),
            &[0],
        )
        .unwrap();
    let mut tx = db.begin().await.unwrap();
    tx.execute("SET LOCAL ROLE oo_pipeline_runtime")
        .await
        .unwrap();
    let owner = "11111111-1111-4111-8111-111111111111";
    let fence: i64 = sqlx::query_scalar("SELECT _oo.adopt_target_generation($1,$2::uuid)")
        .bind(source)
        .bind(owner)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT _oo.full_import_bind_session($1,$2::uuid,$3)")
        .bind(source)
        .bind(owner)
        .bind(fence)
        .execute(&mut *tx)
        .await
        .unwrap();
    let keys: Vec<String> = rows.iter().map(|(k, _)| k.to_string()).collect();
    let missing: Vec<String> = if is_range {
        sqlx::query_scalar("SELECT _oo.c5_range_guard($1,$2::uuid,$3,$4::jsonb,$5)")
            .bind(source)
            .bind(owner)
            .bind(fence)
            .bind(scope.to_string())
            .bind(&keys)
            .fetch_one(&mut *tx)
            .await
            .unwrap()
    } else {
        Vec::new()
    };
    mapping(&mut tx, source, table, generation, Level::Namespace, "").await;
    let physical = mapping(&mut tx, source, table, generation, Level::Table, "").await;
    let key_pc = mapping(&mut tx, source, table, generation, Level::Column, "id").await;
    let value_pc = mapping(&mut tx, source, table, generation, Level::Column, "value").await;
    let digest = "a".repeat(64);
    let arrow = scope["arrow_sha256"].as_str().unwrap();
    let run:String=sqlx::query_scalar("SELECT _oo.full_import_open_run($1,$2::uuid,$3,$4,$5,$6,$6,$7,$6,ARRAY['app'],ARRAY[$8],ARRAY[$9])::text")
        .bind(source).bind(owner).bind(fence).bind(source).bind(generation).bind(&digest).bind(arrow).bind(table).bind(&physical).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("SELECT _oo.full_import_prepare_shadow($1,$2::uuid,$3,$4::uuid,0,$1,$5,$6,'app',$7,ARRAY['id','value'],ARRAY[$8,25]::bigint[],ARRAY[-1,-1],ARRAY[true,true])")
        .bind(source).bind(owner).bind(fence).bind(&run).bind(&digest).bind(generation).bind(table).bind(if text_key{1043i64}else{20i64}).execute(&mut *tx).await.unwrap();
    let copied: i64 = if generation > 1 && is_range {
        sqlx::query_scalar("SELECT _oo.native_update_copy_forward($1,$2::uuid,$3,$1,$4,'app',$5,$6,$7,ARRAY['id','value'],$8,$9,ARRAY['id'])")
        .bind(source).bind(owner).bind(fence).bind(&digest).bind(table).bind(generation-1).bind(generation).bind(&missing).bind(&keys).fetch_one(&mut *tx).await.unwrap()
    } else {
        0
    };
    for (key, value) in rows {
        let sql = format!(
            "INSERT INTO _oo.\"{physical}\"(\"_oo_pos\",\"{key_pc}\",\"{value_pc}\") VALUES(0,$1::text::{},$2)",
            if text_key { "varchar" } else { "bigint" }
        );
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(*key)
            .bind(*value)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let count = copied + rows.len() as i64;
    let batch = format!("batch{generation}");
    sqlx::query("SELECT _oo.full_import_write_batch($1,$2::uuid,$3,$4::uuid,0,$1,$5,$6,$7,$7,$7,$8,$7,$9,0,$10,0,ARRAY[]::text[],ARRAY[]::bigint[],ARRAY[]::integer[],ARRAY[]::bigint[],ARRAY[]::text[])")
        .bind(source).bind(owner).bind(fence).bind(&run).bind(&physical).bind(&batch).bind(&digest).bind(oo_pipeline::full_import::hook_digest()).bind(arrow).bind(count).execute(&mut *tx).await.unwrap();
    sqlx::query("SELECT _oo.full_import_note_table($1,$2::uuid,$3,$4::uuid,0,$5,$6,$1)")
        .bind(source)
        .bind(owner)
        .bind(fence)
        .bind(&run)
        .bind(count)
        .bind(&digest)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT _oo.native_update_seal_quality($1,$2::uuid,$3,$4::uuid,$1,$5,'app',$6,$7,$8,$9,0,'snapshot-zero/v1','fixture','c5_diff','reconcile','B',false,$10,$11,'ok',0,$12)")
        .bind(source).bind(owner).bind(fence).bind(&run).bind(generation).bind(table).bind(&physical).bind(&batch).bind(&digest).bind(&missing).bind(count).bind(if is_range {"incremental"}else{"full"}).execute(&mut *tx).await.unwrap();
    tx.execute("SET CONSTRAINTS ALL IMMEDIATE").await.unwrap();
    sqlx::query("SELECT _oo.full_import_seal_run($1,$2::uuid,$3,$4::uuid,$1,$5,$6,$6,$7,$6,ARRAY['app'],ARRAY[$8])")
        .bind(source).bind(owner).bind(fence).bind(&run).bind(generation).bind(&digest).bind(arrow).bind(table).execute(&mut *tx).await.unwrap();
    if is_range {
        sqlx::query("SELECT _oo.c5_range_commit($1,$2::uuid,$3,$4::jsonb,$5,$6::uuid,$7,$8)")
            .bind(source)
            .bind(owner)
            .bind(fence)
            .bind(scope.to_string())
            .bind(&keys)
            .bind(&run)
            .bind(&batch)
            .bind(&digest)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    if fail {
        assert!(sqlx::query("SELECT 1/0").execute(&mut *tx).await.is_err());
        tx.rollback().await.unwrap();
    } else {
        tx.commit().await.unwrap();
    }
    sqlx::query("SELECT _oo.native_update_release_fence($1)")
        .bind(source)
        .execute(db)
        .await
        .unwrap();
    (scope, physical)
}

fn sha(value: &Value) -> String {
    ti::sha256_hex(serde_json::to_string(value).unwrap().as_bytes())
}

async fn fixture(db: &mut PgConnection) -> (Value, Value) {
    for shard in 0..3 {
        for table in 0..12 {
            load_inner(
                db,
                &format!("lib_s{shard}"),
                &format!("t_{shard}_{table:02}"),
                (table + 1, 0),
                &[("1", "anonymous")],
                (false, false, false),
            )
            .await;
        }
    }
    let mut shards = Vec::new();
    let mut sources = Vec::new();
    for shard in 0..3 {
        let source = format!("lib_s{shard}");
        let tables: String = sqlx::query_scalar("SELECT jsonb_agg(jsonb_build_object('schema',t.schema_ident,'table',t.table_ident,'run_id',r.run_id,'physical_name',t.physical_name,'accepted_rows',t.applied_rows,'canonical_digest',t.canonical_digest,'source_generation',r.source_generation,'target_generation',r.target_generation,'applied_pos',t.applied_pos::text,'capture_method','c5_diff','delete_detection','reconcile','consistency_grade','B') ORDER BY t.table_ident)::text FROM _oo.full_import_run r JOIN _oo.full_import_table t USING(run_id) JOIN _oo.table_freshness f ON f.source=r.source_id AND f.physical_name=t.physical_name WHERE r.source_id=$1")
            .bind(&source).fetch_one(&mut *db).await.unwrap();
        let mut tables: Value = serde_json::from_str(&tables).unwrap();
        for table in tables.as_array_mut().unwrap() {
            table["rejects"] = json!(0);
            table["position_lineage"] = json!("full-import.snapshot");
        }
        let watermark: String = sqlx::query_scalar(
            "SELECT to_jsonb(w)::text FROM _oo.source_watermark w WHERE source=$1",
        )
        .bind(&source)
        .fetch_one(&mut *db)
        .await
        .unwrap();
        let w: Value = serde_json::from_str(&watermark).unwrap();
        let anchor = tables.as_array().unwrap().last().unwrap()["run_id"].clone();
        let receipt = json!({"protocol":"oo.pipeline.native-publication-receipt/v1","collection_mode":"per_table",
            "run_id":anchor,"run_ids":tables.as_array().unwrap().iter().map(|t|t["run_id"].clone()).collect::<Vec<_>>(),
            "source_id":source,"source_generation":w["source_generation"],"target_generation":w["target_generation"],
            "position_codec":w["position_codec"],"position_lineage":w["position_lineage"],"applied_pos":"0",
            "consistency":"per_table","consistency_grade":"B","cross_table_consistent":false,"tables":tables});
        sources.push(json!({"source_id":source,"schema":"app","tables":tables.as_array().unwrap().iter().map(|t|t["table"].clone()).collect::<Vec<_>>()}));
        shards.push(json!({"source_id":source,"anchor_run":anchor,"receipt_sha256":sha(&receipt),"receipt":receipt}));
    }
    let manifest = json!({"protocol":"oo.library.membership/v1","library_id":"anonymous-library","sources":sources});
    (
        json!({"protocol":"oo.pipeline.native-library-publication/v1","library_id":"anonymous-library",
        "manifest_sha256":sha(&manifest),"manifest":manifest,"expected_tables":36,
        "shards":["lib_s0","lib_s1","lib_s2"]}),
        json!(shards),
    )
}

async fn publish(
    db: &mut PgConnection,
    library: &Value,
    shards: &Value,
) -> Result<String, sqlx::Error> {
    let digest = sha(&json!({"library":library,"shards":shards}));
    sqlx::query_scalar("SELECT _oo.publish_native_library($1::jsonb,$2,$3::jsonb)::text")
        .bind(library.to_string())
        .bind(digest)
        .bind(shards.to_string())
        .fetch_one(db)
        .await
}

#[tokio::test]
#[ignore = "live fixture: requires isolated owned PostgreSQL and module fixture configuration; run with --ignored"]
async fn library_atomic_publication_and_adoption() {
    let mut admin = connect("postgres").await;
    admin
        .execute("SELECT pg_advisory_lock(6404202662::bigint)")
        .await
        .unwrap();
    let database = format!(
        "oo_wf_lib_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    admin
        .execute(sqlx::AssertSqlSafe(format!("CREATE DATABASE {database}")))
        .await
        .unwrap();
    let mut db = connect(&database).await;
    install(&mut db).await;
    ti::install_native_library_publication(&mut db)
        .await
        .unwrap();
    ti::install_native_library_publication(&mut db)
        .await
        .unwrap();
    let (library, shards) = fixture(&mut db).await;
    let allowed:bool=sqlx::query_scalar("SELECT has_function_privilege('oo_pipeline_runtime','_oo.publish_native_library(jsonb,text,jsonb)','EXECUTE')").fetch_one(&mut db).await.unwrap();
    assert!(!allowed);
    db.execute("SELECT _oo.configure_native_library_limits(2,35)")
        .await
        .unwrap();
    assert!(publish(&mut db, &library, &shards).await.is_err());
    db.execute("SELECT _oo.configure_native_library_limits(64,4096)")
        .await
        .unwrap();
    let mut omitted_library = library.clone();
    let mut omitted_shards = shards.clone();
    omitted_library["manifest"]["sources"][2]["tables"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    omitted_library["manifest_sha256"] = json!(sha(&omitted_library["manifest"]));
    omitted_library["expected_tables"] = json!(35);
    let removed = omitted_shards[2]["receipt"]["tables"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    omitted_shards[2]["receipt"]["run_ids"]
        .as_array_mut()
        .unwrap()
        .retain(|id| *id != removed["run_id"]);
    omitted_shards[2]["receipt_sha256"] = json!(sha(&omitted_shards[2]["receipt"]));
    assert!(
        publish(&mut db, &omitted_library, &omitted_shards)
            .await
            .is_err(),
        "cannot omit a current table even with a matching reduced manifest"
    );
    let mut unknown = shards.clone();
    unknown[1]["receipt"]["unrecognized"] = json!(true);
    unknown[1]["receipt_sha256"] = json!(sha(&unknown[1]["receipt"]));
    assert!(publish(&mut db, &library, &unknown).await.is_err());
    let mut missing = shards.clone();
    missing.as_array_mut().unwrap().pop();
    assert!(publish(&mut db, &library, &missing).await.is_err());
    let mut bad = shards.clone();
    bad[2]["receipt"]["tables"][0]["accepted_rows"] = json!(999);
    bad[2]["receipt_sha256"] = json!(sha(&bad[2]["receipt"]));
    let error = publish(&mut db, &library, &bad).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("OO030")
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo.native_per_table_publication")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "third shard failure must roll back earlier shards"
    );
    let mut unicode = library.clone();
    unicode["library_id"] = json!("anonymous-库");
    unicode["manifest"]["library_id"] = json!("anonymous-库");
    unicode["manifest_sha256"] = json!(sha(&unicode["manifest"]));
    db.execute("BEGIN").await.unwrap();
    assert!(
        publish(&mut db, &unicode, &shards).await.is_ok(),
        "UTF-8 canonical library identity agrees across Rust and PostgreSQL"
    );
    db.execute("ROLLBACK").await.unwrap();
    let first = &shards[0];
    sqlx::query("SELECT _oo.publish_native_per_table_collection($1::uuid,$2,$3::jsonb)")
        .bind(first["anchor_run"].as_str().unwrap())
        .bind(first["receipt_sha256"].as_str().unwrap())
        .bind(first["receipt"].to_string())
        .execute(&mut db)
        .await
        .unwrap();
    let published: Value =
        serde_json::from_str(&publish(&mut db, &library, &shards).await.unwrap()).unwrap();
    assert_eq!(published["outcome"], "published");
    let replay: Value =
        serde_json::from_str(&publish(&mut db, &library, &shards).await.unwrap()).unwrap();
    assert_eq!(replay["outcome"], "replay");
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM _oo.native_library_source_status WHERE status='superseded_by_library'").fetch_one(&mut db).await.unwrap();
    assert_eq!(count, 3);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo.table_freshness WHERE state='live'")
            .fetch_one(&mut db)
            .await
            .unwrap();
    assert_eq!(count, 36);
    db.execute(include_str!(
        "../../../tools/release-coordinator/migrations/oo_publish_target_v1.sql"
    ))
    .await
    .unwrap();
    db.execute("ALTER SCHEMA oo_publish_target OWNER TO oo_target_owner; ALTER TABLE oo_publish_target.fence OWNER TO oo_target_owner; DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_r_lib_fixture') THEN CREATE ROLE oo_r_lib_fixture NOLOGIN; END IF; END $$; GRANT USAGE ON SCHEMA _oo TO oo_r_lib_fixture;").await.unwrap();
    db.execute("DO $$ DECLARE t record; column_name text; BEGIN FOR t IN SELECT r.source_id,r.source_generation,ft.* FROM _oo.full_import_run r JOIN _oo.full_import_table ft USING(run_id) LOOP SELECT physical_name INTO column_name FROM _oo.physical_mappings WHERE source=t.source_id AND run_generation=t.source_generation AND level='c' AND logical_table=t.table_ident AND logical_column='id'; EXECUTE format('GRANT SELECT (%I) ON _oo.%I TO oo_r_lib_fixture',column_name,t.physical_name); EXECUTE format('ALTER TABLE _oo.%I ENABLE ROW LEVEL SECURITY',t.physical_name); EXECUTE format('CREATE POLICY oo_fence ON _oo.%I FOR SELECT TO oo_r_lib_fixture USING(true)',t.physical_name); INSERT INTO oo_publish_target.fence(singleton,role_name,schema_name,relation_name,state,opened_columns,opened_pack_digest,opened_version,streams_stopped) VALUES(true,'oo_r_lib_fixture','_oo',t.physical_name,'open',ARRAY[column_name],repeat('a',64),'version-1',0); END LOOP; END $$").await.unwrap();
    let selected:(String,String)=sqlx::query_as("SELECT t.physical_name,m.physical_name FROM _oo.full_import_table t JOIN _oo.full_import_run r USING(run_id) JOIN _oo.physical_mappings m ON m.source=r.source_id AND m.run_generation=r.source_generation AND m.level='c' AND m.logical_table=t.table_ident AND m.logical_column='id' LIMIT 1").fetch_one(&mut db).await.unwrap();
    let mut observer = connect(&database).await;
    observer.execute("SET ROLE oo_r_lib_fixture").await.unwrap();
    observer
        .execute(sqlx::AssertSqlSafe(format!(
            "SELECT {} FROM _oo.{}",
            selected.1, selected.0
        )))
        .await
        .unwrap();
    observer.execute("RESET ROLE").await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo.native_library_publication")
        .fetch_one(&mut observer)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert!(
        db.execute("UPDATE _oo.native_library_publication SET library_id='changed'")
            .await
            .is_err()
    );
    assert!(
        db.execute("DELETE FROM _oo.native_library_publication")
            .await
            .is_err()
    );
    assert!(
        db.execute("TRUNCATE _oo.native_library_publication")
            .await
            .is_err()
    );
    sqlx::query("SELECT _oo.native_library_mark_ready('anonymous-library',$1)")
        .bind(sha(&json!({"library":library,"shards":shards})))
        .execute(&mut db)
        .await
        .unwrap();
    let ready: bool = sqlx::query_scalar("SELECT _oo.native_library_source_ready('lib_s0',$1)")
        .bind(shards[0]["receipt_sha256"].as_str().unwrap())
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert!(ready);
    sqlx::query("SELECT _oo.native_library_fail_closed('anonymous-library',$1)")
        .bind(sha(&json!({"library":library,"shards":shards})))
        .execute(&mut db)
        .await
        .unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM _oo.native_library_head")
        .fetch_one(&mut observer)
        .await
        .unwrap();
    assert_eq!(state, "closed");
    let open: i64 =
        sqlx::query_scalar("SELECT count(*) FROM oo_publish_target.fence WHERE state<>'closed'")
            .fetch_one(&mut observer)
            .await
            .unwrap();
    assert_eq!(
        open, 0,
        "post-commit visibility failure closes every table fence"
    );
    observer.execute("SET ROLE oo_r_lib_fixture").await.unwrap();
    assert!(
        observer
            .execute(sqlx::AssertSqlSafe(format!(
                "SELECT {} FROM _oo.{}",
                selected.1, selected.0
            )))
            .await
            .is_err()
    );
    observer.execute("RESET ROLE").await.unwrap();
    load_inner(
        &mut db,
        "lib_s2",
        "t_2_11",
        (13, 0),
        &[("1", "new")],
        (false, false, false),
    )
    .await;
    assert!(
        publish(&mut db, &library, &shards).await.is_err(),
        "old vector cannot resurrect"
    );
    ti::install_native_library_existing_receipt_fields(&mut db)
        .await
        .unwrap();
    assert!(db.execute("UPDATE _oo.schema_migrations SET checksum=repeat('b',64) WHERE version='native-library-existing-receipt-fields/v1'").await.is_err(),"migration markers stay immutable");
    let drift_database = format!("{database}_drift");
    admin
        .execute(sqlx::AssertSqlSafe(format!(
            "CREATE DATABASE {drift_database}"
        )))
        .await
        .unwrap();
    let mut drift = connect(&drift_database).await;
    install_until(&mut drift, false).await;
    drift.execute("INSERT INTO _oo.schema_migrations(version,checksum,phase,filename) VALUES('native-library-existing-receipt-fields/v1',repeat('b',64),'expand','0064_native_library_existing_receipt_fields.sql')").await.unwrap();
    assert!(
        ti::install_native_library_existing_receipt_fields(&mut drift)
            .await
            .is_err(),
        "installer refuses an already recorded mismatched checksum"
    );
}

#[test]
fn library_installer_is_additive_and_ordered() {
    let installer = include_str!("../src/native_update.rs");
    assert!(
        installer
            .find("install_native_c5_cells_array(&mut conn)")
            .unwrap()
            < installer
                .find("install_native_library_publication(&mut conn)")
                .unwrap()
    );
    assert_eq!(
        ti::compiled_native_library_publication_digest(),
        ti::sha256_hex(include_bytes!(
            "../migrations/0061_native_library_publication.sql"
        ))
    );
}

#[tokio::test]
#[ignore = "live fixture: requires isolated owned PostgreSQL and module fixture configuration; run with --ignored"]
async fn library_export_coordinator_fixture() {
    let mut admin = connect("postgres").await;
    admin
        .execute("SELECT pg_advisory_lock(6404202662::bigint)")
        .await
        .unwrap();
    let database = format!("oo_wf_lib_release_{}", std::process::id());
    admin
        .execute(sqlx::AssertSqlSafe(format!("CREATE DATABASE {database}")))
        .await
        .unwrap();
    let mut db = connect(&database).await;
    install(&mut db).await;
    let (library, shards) = fixture(&mut db).await;
    db.execute("DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_target_admin') THEN CREATE ROLE oo_target_admin LOGIN; END IF; IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_library_reader') THEN CREATE ROLE oo_library_reader LOGIN NOINHERIT; END IF; IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_r_lib_all') THEN CREATE ROLE oo_r_lib_all NOLOGIN NOINHERIT; END IF; IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_r_lib_old') THEN CREATE ROLE oo_r_lib_old NOLOGIN NOINHERIT; END IF; END $$; ALTER ROLE oo_library_reader NOINHERIT; ALTER ROLE oo_r_lib_all NOINHERIT; ALTER ROLE oo_r_lib_old NOINHERIT; GRANT oo_target_owner TO oo_target_admin WITH INHERIT TRUE, SET TRUE; GRANT pg_signal_backend TO oo_target_admin; GRANT oo_r_lib_all TO oo_library_reader WITH INHERIT FALSE, SET TRUE; GRANT USAGE ON SCHEMA _oo TO oo_target_admin,oo_library_reader; GRANT SELECT ON _oo.full_import_run,_oo.full_import_table,_oo.physical_mappings TO oo_library_reader").await.unwrap();
    db.execute(sqlx::AssertSqlSafe(format!(
        "GRANT CREATE,CONNECT ON DATABASE {database} TO oo_target_admin; GRANT CONNECT ON DATABASE {database} TO oo_library_reader"
    )))
    .await
    .unwrap();
    let mut bindings = Vec::new();
    for shard in shards.as_array().unwrap() {
        for table in shard["receipt"]["tables"].as_array().unwrap() {
            let columns:Vec<String>=sqlx::query_scalar("SELECT physical_name FROM _oo.physical_mappings WHERE source=$1 AND run_generation=$2 AND level='c' AND logical_table=$3 ORDER BY logical_column")
                .bind(shard["source_id"].as_str().unwrap()).bind(table["source_generation"].as_i64().unwrap()).bind(table["table"].as_str().unwrap()).fetch_all(&mut db).await.unwrap();
            bindings.push(json!([
                "_oo",
                table["physical_name"],
                "oo_r_lib_all",
                columns
            ]));
        }
    }
    std::fs::write("/tmp/oo-wf-lib-release-fixture.json",serde_json::to_vec_pretty(&json!({"database":database,"receipt":{"library":library,"shards":shards},"bindings":bindings})).unwrap()).unwrap();
}
