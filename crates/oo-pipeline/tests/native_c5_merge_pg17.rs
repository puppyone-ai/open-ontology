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
    db.execute("CREATE SCHEMA jobs; CREATE TABLE jobs.tasks(id uuid PRIMARY KEY,state text,generation bigint,lease_until timestamptz,project_id uuid);  GRANT USAGE ON SCHEMA jobs TO oo_target_owner,oo_pipeline_runtime; GRANT SELECT ON jobs.tasks TO oo_target_owner").await.unwrap();
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
}

async fn through_0052(db: &mut PgConnection) {
    install(db).await;
    ti::install_native_c5_range(db).await.unwrap();
    ti::install_native_c5_range_apply(db).await.unwrap();
    ti::install_native_c5_scan(db).await.unwrap();
    ti::install_native_c5_replay_guard(db).await.unwrap();
    ti::install_native_c5_full_initialize(db).await.unwrap();
    ti::install_native_c5_closed_observation(db).await.unwrap();
}

async fn through_0057(db: &mut PgConnection) {
    assert!(ti::install_native_c5_publication(db).await.is_err());
    ti::install_native_c5_multipart(db).await.unwrap();
    ti::install_masked_rejects(db).await.unwrap();
    ti::install_multipart_cells(db).await.unwrap();
    ti::install_native_c5_publication(db).await.unwrap();
    ti::install_native_c5_single_baseline(db).await.unwrap();
    ti::install_native_c5_multipart(db).await.unwrap();
    ti::install_masked_rejects(db).await.unwrap();
    ti::install_multipart_cells(db).await.unwrap();
    ti::install_native_c5_publication(db).await.unwrap();
    ti::install_native_c5_single_baseline(db).await.unwrap();
    for (version, filename, digest) in [
        (
            ti::VERSION_NATIVE_C5_MULTIPART,
            "0053_native_c5_multipart.sql",
            ti::compiled_native_c5_multipart_digest(),
        ),
        (
            ti::VERSION_MASKED_REJECTS,
            "0054_masked_rejects.sql",
            ti::compiled_masked_rejects_digest(),
        ),
        (
            ti::VERSION_MULTIPART_CELLS,
            "0055_multipart_cells.sql",
            ti::compiled_multipart_cells_digest(),
        ),
        (
            ti::VERSION_NATIVE_C5_PUBLICATION,
            "0056_native_c5_publication.sql",
            ti::compiled_native_c5_publication_digest(),
        ),
        (
            ti::VERSION_NATIVE_C5_SINGLE_BASELINE,
            "0057_native_c5_single_baseline.sql",
            ti::compiled_native_c5_single_baseline_digest(),
        ),
    ] {
        let row =
            sqlx::query("SELECT filename,checksum FROM _oo.schema_migrations WHERE version=$1")
                .bind(version)
                .fetch_one(&mut *db)
                .await
                .unwrap();
        assert_eq!(row.get::<String, _>("filename"), filename);
        assert_eq!(row.get::<String, _>("checksum"), digest);
    }
    let dml: bool = sqlx::query_scalar("SELECT EXISTS(SELECT FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo' AND c.relkind='r' AND (has_table_privilege('oo_pipeline_runtime',c.oid,'INSERT') OR has_table_privilege('oo_pipeline_runtime',c.oid,'UPDATE') OR has_table_privilege('oo_pipeline_runtime',c.oid,'DELETE')))")
        .fetch_one(&mut *db).await.unwrap();
    assert!(!dml);
}

#[tokio::test]
#[ignore = "live fixture: requires isolated owned PostgreSQL and module fixture configuration; run with --ignored"]
async fn merged_migrations_fresh_and_upgrade() {
    let mut admin = connect("postgres").await;
    let version: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert!(version.starts_with("17."));
    admin
        .execute("CREATE ROLE oo_jobs_owner NOLOGIN; CREATE ROLE oo_jobs_runtime NOLOGIN")
        .await
        .unwrap();
    for name in ["merge_fresh", "merge_upgrade"] {
        admin
            .execute(match name {
                "merge_fresh" => "CREATE DATABASE merge_fresh",
                _ => "CREATE DATABASE merge_upgrade",
            })
            .await
            .unwrap();
        let mut db = connect(name).await;
        through_0052(&mut db).await;
        if name == "merge_upgrade" {
            db.close().await.unwrap();
            db = connect(name).await;
        }
        through_0057(&mut db).await;
    }
}
