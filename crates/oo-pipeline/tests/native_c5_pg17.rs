use oo_pipeline::target_installer as ti;
use sqlx::{Connection, Executor, PgConnection, Row};

async fn connect(db: &str) -> PgConnection {
    let port = std::env::var("OO_C5_PG_PORT").expect("owned PG17 OO_C5_PG_PORT");
    PgConnection::connect(&format!(
        "postgres://postgres:fixture-only@127.0.0.1:{port}/{db}"
    ))
    .await
    .unwrap()
}

async fn install(db: &mut PgConnection) {
    ti::install(db).await.unwrap();
    db.execute("CREATE SCHEMA jobs; CREATE TABLE jobs.tasks(id uuid PRIMARY KEY,state text,generation bigint,lease_until timestamptz,project_id uuid); CREATE ROLE oo_jobs_owner NOLOGIN; CREATE ROLE oo_jobs_runtime NOLOGIN; GRANT USAGE ON SCHEMA jobs TO oo_target_owner,oo_pipeline_runtime; GRANT SELECT ON jobs.tasks TO oo_target_owner").await.unwrap();
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
    // 0043 holds the exact tombstone count that the batch guard calls (T8).
    ti::install_native_c1_chunked_baseline_final(db)
        .await
        .expect("0043");
    ti::install_native_source_instance_grant(db).await.unwrap();
    ti::install_native_c1_ack_seed_format(db).await.unwrap();
}

#[tokio::test]
#[ignore = "live fixture: requires isolated owned PostgreSQL and module fixture configuration; run with --ignored"]
async fn pg17_c5_lifecycle() {
    let mut admin = connect("postgres").await;
    let version: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert!(version.starts_with("17."));
    admin
        .execute("DROP DATABASE IF EXISTS c5_range WITH (FORCE)")
        .await
        .unwrap();
    admin.execute("DROP ROLE IF EXISTS oo_pipeline_runtime; DROP ROLE IF EXISTS oo_target_owner; DROP ROLE IF EXISTS oo_jobs_runtime; DROP ROLE IF EXISTS oo_jobs_owner").await.unwrap();
    admin.execute("CREATE DATABASE c5_range").await.unwrap();
    let mut db = connect("c5_range").await;
    install(&mut db).await;
    ti::install_native_c5_range(&mut db).await.unwrap();
    ti::install_native_c5_range(&mut db).await.unwrap();
    ti::install_native_c5_range_apply(&mut db).await.unwrap();
    ti::install_native_c5_scan(&mut db).await.unwrap();
    ti::install_native_c5_replay_guard(&mut db).await.unwrap();
    ti::install_native_c5_full_initialize(&mut db)
        .await
        .unwrap();
    ti::install_native_c5_closed_observation(&mut db)
        .await
        .unwrap();
    let mut tx = db.begin().await.unwrap();
    let fence: i64 = sqlx::query_scalar("SELECT _oo.adopt_target_generation('c5_fixture','11111111-1111-4111-8111-111111111111'::uuid)").fetch_one(&mut *tx).await.unwrap();
    sqlx::query("SELECT _oo.full_import_bind_session('c5_fixture','11111111-1111-4111-8111-111111111111'::uuid,$1)").bind(fence).execute(&mut *tx).await.unwrap();
    let scope = serde_json::json!({"protocol":"oo.pipeline.native-c5-range/v1","source_engine":"mysql","capture_method":"c5_diff","source_id":"fixture","schema":"app","table":"t_orders","schema_fingerprint":"a".repeat(64),"chunk_topology_revision":1,"chunk_id":"0","lo":null,"hi":[{"kind":"int","value":10}],"prior_membership_revision":0,"parent_generation":0,"source_generation":1,"arrow_sha256":"b".repeat(64),"row_count":1,"delete_detection":"reconcile","position_codec":"snapshot-zero/v1","applied_pos":"0","members":[[{"kind":"int","value":1}]]});
    let missing:Vec<String>=sqlx::query_scalar("SELECT _oo.c5_range_guard('c5_fixture','11111111-1111-4111-8111-111111111111'::uuid,$1,$2::jsonb,ARRAY['1'])").bind(fence).bind(scope.to_string()).fetch_one(&mut *tx).await.unwrap();
    assert!(missing.is_empty());
    assert!(sqlx::query("SELECT _oo.c5_range_commit('c5_fixture','11111111-1111-4111-8111-111111111111'::uuid,$1,$2::jsonb,ARRAY['1'],gen_random_uuid(),'batch',repeat('a',64))").bind(fence).bind(scope.to_string()).execute(&mut *tx).await.is_err());
    tx.rollback().await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo.c5_chunk")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(n, 0);
    for (source, table, text_key) in [
        ("int_source", "t_int", false),
        ("text_source", "t_text", true),
    ] {
        let keys = if text_key { ["Z ", "é"] } else { ["1", "2"] };
        let (_, initial_physical) = load(
            &mut db,
            source,
            table,
            (1, 0),
            &[(keys[0], "old"), (keys[1], "keep")],
            text_key,
            false,
        )
        .await;
        db.execute("BEGIN").await.unwrap();
        let fence: i64 = sqlx::query_scalar(
            "SELECT _oo.adopt_target_generation($1,'11111111-1111-4111-8111-111111111111'::uuid)",
        )
        .bind(source)
        .fetch_one(&mut db)
        .await
        .unwrap();
        sqlx::query("SELECT _oo.full_import_bind_session($1,'11111111-1111-4111-8111-111111111111'::uuid,$2)").bind(source).bind(fence).execute(&mut db).await.unwrap();
        sqlx::query("SELECT _oo.c5_scan_begin($1,'11111111-1111-4111-8111-111111111111'::uuid,$2,$1,'app',$3,'scan-1','[\"0\"]')").bind(source).bind(fence).bind(table).execute(&mut db).await.unwrap();
        db.execute("COMMIT").await.unwrap();
        sqlx::query("SELECT _oo.native_update_release_fence($1)")
            .bind(source)
            .execute(&mut db)
            .await
            .unwrap();
        let before: String =
            sqlx::query_scalar("SELECT _oo.c5_authority_export($1,'app',$2)::text")
                .bind(source)
                .bind(table)
                .fetch_one(&mut db)
                .await
                .unwrap();
        load(
            &mut db,
            source,
            table,
            (2, 1),
            &[(keys[0], "updated")],
            text_key,
            true,
        )
        .await;
        let after: String = sqlx::query_scalar("SELECT _oo.c5_authority_export($1,'app',$2)::text")
            .bind(source)
            .bind(table)
            .fetch_one(&mut db)
            .await
            .unwrap();
        assert_eq!(before, after);
        let values = read_values(&mut db, source, table, 1, &initial_physical).await;
        assert_eq!(
            values.into_iter().collect::<std::collections::HashSet<_>>(),
            ["old".to_string(), "keep".to_string()]
                .into_iter()
                .collect()
        );
        let failed_runs: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _oo.full_import_run WHERE source_id=$1 AND source_generation=2",
        )
        .bind(source)
        .fetch_one(&mut db)
        .await
        .unwrap();
        assert_eq!(failed_runs, 0);
        let failed_mappings: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _oo.physical_mappings WHERE source=$1 AND run_generation=2",
        )
        .bind(source)
        .fetch_one(&mut db)
        .await
        .unwrap();
        assert_eq!(failed_mappings, 0);
        let n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM _oo.c5_block_receipt WHERE source=$1")
                .bind(source)
                .fetch_one(&mut db)
                .await
                .unwrap();
        assert_eq!(n, 1);
        let (old_scope, updated_physical) = load(
            &mut db,
            source,
            table,
            (2, 1),
            &[(keys[0], "updated")],
            text_key,
            false,
        )
        .await;
        assert_eq!(
            read_values(&mut db, source, table, 2, &updated_physical).await,
            vec!["updated"]
        );
        let (_, empty_physical) = load(&mut db, source, table, (3, 2), &[], text_key, false).await;
        assert!(
            read_values(&mut db, source, table, 3, &empty_physical)
                .await
                .is_empty()
        );
        reject_old(&mut db, source, &old_scope, &[keys[0].into()]).await;
        assert!(
            read_values(&mut db, source, table, 3, &empty_physical)
                .await
                .is_empty()
        );
        let (last_scope, physical) = load(
            &mut db,
            source,
            table,
            (4, 3),
            &[(keys[0], "reinsert")],
            text_key,
            false,
        )
        .await;
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM _oo.\"{physical}\""
        )))
        .fetch_one(&mut db)
        .await
        .unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            read_values(&mut db, source, table, 4, &physical).await,
            vec!["reinsert"]
        );
        let n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM _oo.update_tombstone WHERE source=$1")
                .bind(source)
                .fetch_one(&mut db)
                .await
                .unwrap();
        assert_eq!(n, 2);
        let mut tx = db.begin().await.unwrap();
        let fence: i64 = sqlx::query_scalar(
            "SELECT _oo.adopt_target_generation($1,'11111111-1111-4111-8111-111111111111'::uuid)",
        )
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        sqlx::query("SELECT _oo.full_import_bind_session($1,'11111111-1111-4111-8111-111111111111'::uuid,$2)").bind(source).bind(fence).execute(&mut *tx).await.unwrap();
        let cursor:i64=sqlx::query_scalar("SELECT _oo.c5_scan_begin($1,'11111111-1111-4111-8111-111111111111'::uuid,$2,$1,'app',$3,'scan-1','[\"0\"]')").bind(source).bind(fence).bind(table).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(cursor, 1);
        let authority: String =
            sqlx::query_scalar("SELECT _oo.c5_authority_export($1,'app',$2)::text")
                .bind(source)
                .bind(table)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        let mut authority: Value = serde_json::from_str(&authority).unwrap();
        assert_eq!(authority["source_generation"], 4);
        assert_eq!(authority["chunks"][0]["membership_revision"], 4);
        authority["chunks"][0]["digest"] = json!([1, "0123456789abcdef", "fedcba9876543210"]);
        let now: i64 =
            sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        let observation = json!({"schema_fingerprint":"a".repeat(64),"checked_at_ms":now,"full_digest_scan_completed":true,"chunks":authority["chunks"],"recent_window_watermark_ms":now});
        let before: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo.apply_receipts")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT _oo.c5_observe($1,'11111111-1111-4111-8111-111111111111'::uuid,$2,$1,'app',$3,4,$4::jsonb)").bind(source).bind(fence).bind(table).bind(observation.to_string()).execute(&mut *tx).await.unwrap();
        let after: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo.apply_receipts")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(before, after);
        let quality:(Option<String>,bool,bool)=sqlx::query_as("SELECT to_jsonb(q)->>'c5_mode',to_jsonb(q)->>'last_full_digest_scan_completed_at' IS NOT NULL,to_jsonb(q)->>'recent_window_watermark' IS NOT NULL FROM _oo.table_quality q WHERE schema_name='app' AND table_name=$1").bind(table).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(quality, (Some("layered".into()), true, true));
        let receipt:(String,String,String)=sqlx::query_as("SELECT run_id::text,batch_id,payload_digest FROM _oo.c5_block_receipt WHERE source=$1 AND source_generation=4").bind(source).fetch_one(&mut *tx).await.unwrap();
        for _ in 0..2 {
            sqlx::query("SELECT _oo.c5_range_commit($1,'11111111-1111-4111-8111-111111111111'::uuid,$2,$3::jsonb,ARRAY[]::text[],$4::uuid,$5,$6)").bind(source).bind(fence).bind(last_scope.to_string()).bind(&receipt.0).bind(&receipt.1).bind(&receipt.2).execute(&mut *tx).await.unwrap();
        }
        tx.commit().await.unwrap();
        let mut tx = db.begin().await.unwrap();
        let fence: i64 = sqlx::query_scalar(
            "SELECT _oo.adopt_target_generation($1,'11111111-1111-4111-8111-111111111111'::uuid)",
        )
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        sqlx::query("SELECT _oo.full_import_bind_session($1,'11111111-1111-4111-8111-111111111111'::uuid,$2)").bind(source).bind(fence).execute(&mut *tx).await.unwrap();
        assert!(sqlx::query("SELECT _oo.c5_range_guard($1,'11111111-1111-4111-8111-111111111111'::uuid,$2,$3::jsonb,ARRAY[]::text[])").bind(source).bind(fence).bind(last_scope.to_string()).execute(&mut *tx).await.is_err());
        tx.rollback().await.unwrap();
        sqlx::query("SELECT _oo.native_update_release_fence($1)")
            .bind(source)
            .execute(&mut db)
            .await
            .unwrap();
        for (field, value) in [
            ("schema_fingerprint", json!("0".repeat(64))),
            ("schema_fingerprint", Value::Null),
            ("source_generation", Value::Null),
            ("chunk_topology_revision", json!(2)),
            ("chunk_id", json!("new")),
            ("lo", json!([{"kind":"text","value":"61"}])),
            ("hi", json!([{"kind":"int","value":9}])),
            ("prior_membership_revision", json!(3)),
            ("parent_generation", json!(3)),
            ("source_generation", json!(6)),
            ("schema", json!("other")),
            ("table", json!("other")),
            ("arrow_sha256", json!("not-a-digest")),
            ("where", json!("true")),
        ] {
            let mut scope = last_scope.clone();
            scope["parent_generation"] = json!(4);
            scope["source_generation"] = json!(5);
            scope["prior_membership_revision"] = json!(4);
            scope["row_count"] = json!(0);
            scope["members"] = json!([]);
            scope[field] = value;
            let mut tx = db.begin().await.unwrap();
            let fence:i64=sqlx::query_scalar("SELECT _oo.adopt_target_generation($1,'11111111-1111-4111-8111-111111111111'::uuid)").bind(source).fetch_one(&mut *tx).await.unwrap();
            sqlx::query("SELECT _oo.full_import_bind_session($1,'11111111-1111-4111-8111-111111111111'::uuid,$2)").bind(source).bind(fence).execute(&mut *tx).await.unwrap();
            assert!(sqlx::query("SELECT _oo.c5_range_guard($1,'11111111-1111-4111-8111-111111111111'::uuid,$2,$3::jsonb,ARRAY[]::text[])").bind(source).bind(fence).bind(scope.to_string()).execute(&mut *tx).await.is_err(),"{field}");
            tx.rollback().await.unwrap();
            sqlx::query("SELECT _oo.native_update_release_fence($1)")
                .bind(source)
                .execute(&mut db)
                .await
                .unwrap();
        }
        let mut tx = db.begin().await.unwrap();
        let fence: i64 = sqlx::query_scalar(
            "SELECT _oo.adopt_target_generation($1,'11111111-1111-4111-8111-111111111111'::uuid)",
        )
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        sqlx::query("SELECT _oo.full_import_bind_session($1,'11111111-1111-4111-8111-111111111111'::uuid,$2)").bind(source).bind(fence).execute(&mut *tx).await.unwrap();
        sqlx::query("SELECT _oo.c5_scan_begin($1,'11111111-1111-4111-8111-111111111111'::uuid,$2,$1,'app',$3,'scan-pending','[\"0\",\"missing\"]')").bind(source).bind(fence).bind(table).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        sqlx::query("SELECT _oo.native_update_release_fence($1)")
            .bind(source)
            .execute(&mut db)
            .await
            .unwrap();
        let mut tx = db.begin().await.unwrap();
        let fence: i64 = sqlx::query_scalar(
            "SELECT _oo.adopt_target_generation($1,'11111111-1111-4111-8111-111111111111'::uuid)",
        )
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        sqlx::query("SELECT _oo.full_import_bind_session($1,'11111111-1111-4111-8111-111111111111'::uuid,$2)").bind(source).bind(fence).execute(&mut *tx).await.unwrap();
        let cursor:i64=sqlx::query_scalar("SELECT _oo.c5_scan_begin($1,'11111111-1111-4111-8111-111111111111'::uuid,$2,$1,'app',$3,'scan-pending','[\"0\",\"missing\"]')").bind(source).bind(fence).bind(table).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(cursor, 0);
        tx.execute("SAVEPOINT null_observation").await.unwrap();
        assert!(sqlx::query("SELECT _oo.c5_observe($1,'11111111-1111-4111-8111-111111111111'::uuid,$2,$1,'app',$3,4,$4::jsonb)").bind(source).bind(fence).bind(table).bind(json!({"schema_fingerprint":"a".repeat(64),"checked_at_ms":now,"full_digest_scan_completed":true}).to_string()).execute(&mut *tx).await.is_err());
        tx.execute("ROLLBACK TO SAVEPOINT null_observation")
            .await
            .unwrap();
        assert!(sqlx::query("SELECT _oo.c5_observe($1,'11111111-1111-4111-8111-111111111111'::uuid,$2,$1,'app',$3,4,$4::jsonb)").bind(source).bind(fence).bind(table).bind(observation.to_string()).execute(&mut *tx).await.is_err());
        tx.rollback().await.unwrap();
        sqlx::query("SELECT _oo.native_update_release_fence($1)")
            .bind(source)
            .execute(&mut db)
            .await
            .unwrap();
        assert_eq!(
            read_values(&mut db, source, table, 4, &physical).await,
            vec!["reinsert"]
        );
    }
    for source in ["legacy_full", "legacy_c5"] {
        let text_key = source == "legacy_c5";
        load_inner(
            &mut db,
            source,
            source,
            (1, 0),
            &[("1", "old"), ("2", "old")],
            (text_key, false, false),
        )
        .await;
        let (_, physical) = load_inner(
            &mut db,
            source,
            source,
            (2, 0),
            &[("1", "updated")],
            (text_key, false, false),
        )
        .await;
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM _oo.\"{physical}\""
        )))
        .fetch_one(&mut db)
        .await
        .unwrap();
        assert_eq!(n, 1);
        let n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM _oo.c5_block_receipt WHERE source=$1")
                .bind(source)
                .fetch_one(&mut db)
                .await
                .unwrap();
        assert_eq!(n, 0);
        let mut tx = db.begin().await.unwrap();
        let fence: i64 = sqlx::query_scalar(
            "SELECT _oo.adopt_target_generation($1,'11111111-1111-4111-8111-111111111111'::uuid)",
        )
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        sqlx::query("SELECT _oo.full_import_bind_session($1,'11111111-1111-4111-8111-111111111111'::uuid,$2)").bind(source).bind(fence).execute(&mut *tx).await.unwrap();
        sqlx::query("SELECT _oo.c5_initialize_full($1,'11111111-1111-4111-8111-111111111111'::uuid,$2,$1,'app',$1,repeat('a',64),1,'0',ARRAY['id'])").bind(source).bind(fence).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        sqlx::query("SELECT _oo.native_update_release_fence($1)")
            .bind(source)
            .execute(&mut db)
            .await
            .unwrap();
        load(&mut db, source, source, (3, 1), &[], text_key, false).await;
        let n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM _oo.update_tombstone WHERE source=$1")
                .bind(source)
                .fetch_one(&mut db)
                .await
                .unwrap();
        assert_eq!(n, 1);
        load_inner(
            &mut db,
            source,
            source,
            (4, 0),
            &[("1", "full-reinsert")],
            (text_key, false, false),
        )
        .await;
        let mut tx = db.begin().await.unwrap();
        let fence: i64 = sqlx::query_scalar(
            "SELECT _oo.adopt_target_generation($1,'11111111-1111-4111-8111-111111111111'::uuid)",
        )
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        sqlx::query("SELECT _oo.full_import_bind_session($1,'11111111-1111-4111-8111-111111111111'::uuid,$2)").bind(source).bind(fence).execute(&mut *tx).await.unwrap();
        sqlx::query("SELECT _oo.c5_initialize_full($1,'11111111-1111-4111-8111-111111111111'::uuid,$2,$1,'app',$1,repeat('a',64),2,'1',ARRAY['id'])").bind(source).bind(fence).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        sqlx::query("SELECT _oo.native_update_release_fence($1)")
            .bind(source)
            .execute(&mut db)
            .await
            .unwrap();
        let authority: String =
            sqlx::query_scalar("SELECT _oo.c5_authority_export($1,'app',$1)::text")
                .bind(source)
                .fetch_one(&mut db)
                .await
                .unwrap();
        let authority: Value = serde_json::from_str(&authority).unwrap();
        assert_eq!(authority["chunks"].as_array().unwrap().len(), 1);
        assert_eq!(authority["chunks"][0]["chunk_id"], "1");
        assert_eq!(authority["chunk_topology_revision"], 2);
    }
    let gateway = include_str!("../../oo-gateway/src/query_executor.rs");
    let arm = gateway.split("\"table_quality\" => {").nth(1).unwrap();
    let sql = arm.split('"').nth(1).unwrap().replace("\\\n", " ");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 4);
    for row in rows
        .into_iter()
        .filter(|row| row.get::<String, _>(1).starts_with("t_"))
    {
        assert_eq!(row.get::<Option<String>, _>(10), Some("layered".into()));
        assert!(row.get::<Option<String>, _>(11).is_some());
        assert!(row.get::<Option<String>, _>(12).is_some());
    }
    let password =
        std::env::temp_dir().join(format!("oo-wf-c5load-export-{}.pw", std::process::id()));
    let output = std::env::temp_dir().join(format!(
        "oo-wf-c5load-authority-{}.json",
        std::process::id()
    ));
    std::fs::write(&password, "fixture-only").unwrap();
    let document = json!({"protocol":"oo.pipeline.native-c5-state/v1","operation":"export","source_id":"int_source","schema":"app","table":"t_int","authority_file":output,"target":{"host":"127.0.0.1","port":std::env::var("OO_C5_PG_PORT").unwrap().parse::<u16>().unwrap(),"database":"c5_range","runtime_principal":"postgres","password_file":password,"ssl":"disable"}});
    let exported = oo_pipeline::native_c5_state::execute(&document)
        .await
        .unwrap();
    assert_eq!(exported["source_generation"], 4);
    assert_eq!(exported["next_chunk_index"], 0);
    assert_eq!(
        exported["chunks"][0]["digest"],
        json!([1, "0123456789abcdef", "fedcba9876543210"])
    );
    let file: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(file, exported);
    use std::io::Write;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_oo-pipeline"))
        .arg("native-c5-state")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&document).unwrap())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    let actual: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(actual, exported);
    std::fs::remove_file(output).unwrap();
    std::fs::remove_file(password).unwrap();
    let functions:Vec<(bool,Vec<String>,String)>=sqlx::query_as("SELECT p.prosecdef,p.proconfig,r.rolname::text FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace JOIN pg_roles r ON r.oid=p.proowner WHERE n.nspname='_oo' AND p.proname LIKE 'c5_%' AND has_function_privilege('oo_pipeline_runtime',p.oid,'EXECUTE')").fetch_all(&mut db).await.unwrap();
    assert_eq!(functions.len(), 6);
    for (definer, config, owner) in functions {
        assert!(definer);
        assert_eq!(owner, "oo_target_owner");
        assert!(
            config
                .iter()
                .any(|entry| entry == "search_path=pg_catalog, _oo")
        );
    }
    ti::install(&mut db).await.unwrap();
    ti::install_native_update(&mut db).await.unwrap();
    ti::install_native_c5_full_initialize(&mut db)
        .await
        .unwrap();
    let columns: i64 = sqlx::query_scalar("SELECT count(*) FROM information_schema.columns WHERE table_schema='_oo' AND table_name='table_quality' AND column_name IN ('c5_mode','last_full_digest_scan_completed_at','recent_window_watermark')").fetch_one(&mut db).await.unwrap();
    assert_eq!(columns, 3);
    let dml: bool = sqlx::query_scalar(
        "SELECT has_table_privilege('oo_pipeline_runtime','_oo.c5_member','INSERT,UPDATE,DELETE')",
    )
    .fetch_one(&mut db)
    .await
    .unwrap();
    assert!(!dml);
    db.execute("ALTER TABLE _oo.schema_migrations DISABLE TRIGGER USER; UPDATE _oo.schema_migrations SET checksum=repeat('0',64) WHERE version='native-c5-range/v1'; ALTER TABLE _oo.schema_migrations ENABLE TRIGGER USER").await.unwrap();
    assert!(ti::install_native_c5_range(&mut db).await.is_err());
}

use oo_pipeline::physical_mapping::{Level, MappingRequest, plan_physical_mapping};
use serde_json::{Value, json};

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

async fn load(
    db: &mut PgConnection,
    source: &str,
    table: &str,
    (generation, revision): (i64, i64),
    rows: &[(&str, &str)],
    text_key: bool,
    fail: bool,
) -> (Value, String) {
    load_inner(
        db,
        source,
        table,
        (generation, revision),
        rows,
        (text_key, fail, true),
    )
    .await
}

async fn read_values(
    db: &mut PgConnection,
    source: &str,
    table: &str,
    generation: i64,
    physical: &str,
) -> Vec<String> {
    let request = MappingRequest {
        source: source.into(),
        config_digest: "a".repeat(64),
        run_generation: generation,
        level: Level::Column,
        logical_namespace: "app".into(),
        logical_table: table.into(),
        logical_column: "value".into(),
    };
    let value = plan_physical_mapping(&request).unwrap().physical_name;
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT \"{value}\"::text FROM _oo.\"{physical}\""
    )))
    .fetch_all(db)
    .await
    .unwrap()
}

async fn reject_old(db: &mut PgConnection, source: &str, scope: &Value, keys: &[String]) {
    let mut tx = db.begin().await.unwrap();
    let fence: i64 = sqlx::query_scalar(
        "SELECT _oo.adopt_target_generation($1,'11111111-1111-4111-8111-111111111111'::uuid)",
    )
    .bind(source)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "SELECT _oo.full_import_bind_session($1,'11111111-1111-4111-8111-111111111111'::uuid,$2)",
    )
    .bind(source)
    .bind(fence)
    .execute(&mut *tx)
    .await
    .unwrap();
    assert!(sqlx::query("SELECT _oo.c5_range_guard($1,'11111111-1111-4111-8111-111111111111'::uuid,$2,$3::jsonb,$4)").bind(source).bind(fence).bind(scope.to_string()).bind(keys).execute(&mut *tx).await.is_err());
    tx.rollback().await.unwrap();
    sqlx::query("SELECT _oo.native_update_release_fence($1)")
        .bind(source)
        .execute(db)
        .await
        .unwrap();
}
