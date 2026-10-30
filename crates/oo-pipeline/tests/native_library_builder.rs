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
    static INSTALL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = INSTALL.lock().await;
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

async fn load_builder_table(
    db: &mut PgConnection,
    source: &str,
    table: &str,
    (generation, revision): (i64, i64),
    rows: &[(&str, &str)],
    columns: &[oo_pipeline::native_update::FixtureColumn],
    values: &[Vec<oo_pipeline::native_update::FixtureCell>],
) -> (Value, String) {
    use oo_pipeline::native_update::{
        FixtureCell, FixtureKind, encode_empty_fixture_arrow, encode_fixture_arrow,
    };
    let text_key = false;
    let is_range = false;
    let fail = false;
    let bytes = if values.is_empty() {
        encode_empty_fixture_arrow(columns)
    } else {
        encode_fixture_arrow(columns, values)
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
    let mut pcs = Vec::new();
    let names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
    let types: Vec<i64> = columns
        .iter()
        .map(|c| {
            if matches!(c.kind, FixtureKind::Int64) {
                20
            } else {
                25
            }
        })
        .collect();
    for column in columns {
        pcs.push(
            mapping(
                &mut tx,
                source,
                table,
                generation,
                Level::Column,
                &column.name,
            )
            .await,
        );
    }
    let digest = "a".repeat(64);
    let arrow = scope["arrow_sha256"].as_str().unwrap();
    let run:String=sqlx::query_scalar("SELECT _oo.full_import_open_run($1,$2::uuid,$3,$4,$5,$6,$6,$7,$6,ARRAY['app'],ARRAY[$8],ARRAY[$9])::text")
        .bind(source).bind(owner).bind(fence).bind(source).bind(generation).bind(&digest).bind(arrow).bind(table).bind(&physical).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("SELECT _oo.full_import_prepare_shadow($1,$2::uuid,$3,$4::uuid,0,$1,$5,$6,'app',$7,$8::text[],$9::bigint[],$10::integer[],$11::boolean[])")
        .bind(source).bind(owner).bind(fence).bind(&run).bind(&digest).bind(generation).bind(table).bind(&names).bind(&types).bind(vec![-1i32;names.len()]).bind(vec![true;names.len()]).execute(&mut *tx).await.unwrap();
    let copied: i64 = if generation > 1 && is_range {
        sqlx::query_scalar("SELECT _oo.native_update_copy_forward($1,$2::uuid,$3,$1,$4,'app',$5,$6,$7,ARRAY['id','value'],$8,$9,ARRAY['id'])")
        .bind(source).bind(owner).bind(fence).bind(&digest).bind(table).bind(generation-1).bind(generation).bind(&missing).bind(&keys).fetch_one(&mut *tx).await.unwrap()
    } else {
        0
    };
    for row in values {
        let targets = pcs
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(",");
        let params = types
            .iter()
            .enumerate()
            .map(|(i, t)| {
                format!(
                    "${}::text::{}",
                    i + 1,
                    if *t == 20 { "bigint" } else { "text" }
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let sql =
            format!("INSERT INTO _oo.\"{physical}\"(\"_oo_pos\",{targets}) VALUES(0,{params})");
        let strings: Vec<String> = row
            .iter()
            .map(|v| match v {
                FixtureCell::Int(v) => v.to_string(),
                FixtureCell::Text(v) => v.clone(),
                _ => panic!("anonymous cell type"),
            })
            .collect();
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for value in &strings {
            query = query.bind(value);
        }
        query.execute(&mut *tx).await.unwrap();
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

#[tokio::test]
async fn builder_library_import_fixture() {
    use oo_pipeline::native_update::{FixtureCell, FixtureColumn, FixtureKind};
    let input = std::env::var("OO_LIBRARY_BUILDER_INVENTORY").expect("owned anonymous inventory");
    let output = std::env::var("OO_LIBRARY_BUILDER_EXPORT").expect("private output");
    let inventory: Value = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    let mut admin = connect("postgres").await;
    let database = format!("oo_yjlib_i2_{}", std::process::id());
    admin
        .execute(sqlx::AssertSqlSafe(format!("CREATE DATABASE {database}")))
        .await
        .unwrap();
    let mut db = connect(&database).await;
    install(&mut db).await;
    let mut generations = std::collections::BTreeMap::new();
    for table in inventory.as_array().unwrap() {
        let source = table["source"].as_str().unwrap();
        let dataset = table["dataset"].as_str().unwrap();
        let name = dataset.strip_prefix("app.").unwrap();
        let count = table["rows"].as_i64().unwrap();
        let columns: Vec<FixtureColumn> = table["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| FixtureColumn {
                name: c[0].as_str().unwrap().into(),
                nullable: false,
                kind: if c[1] == "bigint" {
                    FixtureKind::Int64
                } else {
                    FixtureKind::Utf8
                },
            })
            .collect();
        let values: Vec<Vec<FixtureCell>> = (1..=count)
            .map(|n| {
                columns
                    .iter()
                    .map(|c| {
                        if c.name == table["key"].as_str().unwrap() {
                            FixtureCell::Int(n)
                        } else if c.name == "owner_unit_id" {
                            FixtureCell::Int((n % 3) + 1)
                        } else if c.name == "kind_code" {
                            FixtureCell::Text("status".into())
                        } else if c.name == "status" || c.name == "value_code" {
                            FixtureCell::Text(((n % 3) + 1).to_string())
                        } else if matches!(c.kind, FixtureKind::Utf8) {
                            FixtureCell::Text(format!("fixture-{n}"))
                        } else {
                            FixtureCell::Int(1)
                        }
                    })
                    .collect()
            })
            .collect();
        let rows: Vec<(String, String)> = (1..=count)
            .map(|n| (n.to_string(), "fixture".to_string()))
            .collect();
        let rows: Vec<(&str, &str)> = rows.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let generation = generations.entry(source.to_string()).or_insert(0i64);
        *generation += 1;
        load_builder_table(
            &mut db,
            source,
            name,
            (*generation, 0),
            &rows,
            &columns,
            &values,
        )
        .await;
    }
    let mut sources = Vec::new();
    for source in generations.keys() {
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT table_name FROM _oo.table_freshness WHERE source=$1 ORDER BY table_name",
        )
        .bind(source)
        .fetch_all(&mut db)
        .await
        .unwrap();
        sources.push(json!({"source_id":source,"schema":"app","tables":tables}));
    }
    assert_eq!(sources.len(), 6);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo.table_freshness")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(count, 72);
    db.execute("DO $$ BEGIN IF NOT EXISTS(SELECT FROM pg_roles WHERE rolname='oo_target_admin') THEN CREATE ROLE oo_target_admin LOGIN; END IF; IF NOT EXISTS(SELECT FROM pg_roles WHERE rolname='oo_library_reader') THEN CREATE ROLE oo_library_reader LOGIN NOINHERIT; END IF; IF NOT EXISTS(SELECT FROM pg_roles WHERE rolname='oo_r_lib_all') THEN CREATE ROLE oo_r_lib_all NOLOGIN NOINHERIT; END IF; END $$; GRANT oo_target_owner TO oo_target_admin WITH INHERIT TRUE,SET TRUE; GRANT pg_signal_backend TO oo_target_admin; GRANT oo_r_lib_all TO oo_library_reader WITH INHERIT FALSE,SET TRUE; GRANT USAGE ON SCHEMA _oo TO oo_target_admin,oo_library_reader; GRANT SELECT ON _oo.full_import_run,_oo.full_import_table,_oo.physical_mappings TO oo_library_reader; GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text,text,bigint,text,text,text,text) TO oo_library_reader").await.unwrap();
    db.execute(sqlx::AssertSqlSafe(format!("GRANT CREATE,CONNECT ON DATABASE {database} TO oo_target_admin; GRANT CONNECT ON DATABASE {database} TO oo_library_reader"))).await.unwrap();
    std::fs::write(output,serde_json::to_vec_pretty(&json!({"database":database,"membership":{"protocol":"oo.library.membership/v1","library_id":"int-anonymous-library","sources":sources}})).unwrap()).unwrap();
}

#[tokio::test]
async fn catalog_replay_works_with_transaction_lock_but_no_session_lock_permission() {
    let mut admin = connect("postgres").await;
    let database = format!("oo_yjlib_lock_{}", std::process::id());
    admin
        .execute(sqlx::AssertSqlSafe(format!("CREATE DATABASE {database}")))
        .await
        .unwrap();
    let mut db = connect(&database).await;
    install(&mut db).await;
    db.execute("REVOKE EXECUTE ON FUNCTION pg_catalog.pg_advisory_lock(bigint),pg_catalog.pg_advisory_unlock(bigint) FROM PUBLIC,oo_target_admin,oo_target_owner; GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(bigint) TO oo_target_admin,oo_target_owner;").await.unwrap();
    let before: String = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(m) ORDER BY version,phase)::text FROM _oo.schema_migrations m",
    )
    .fetch_one(&mut db)
    .await
    .unwrap();
    let port = std::env::var("OO_LIBRARY_PG_PORT").unwrap();
    let mut restricted = PgConnection::connect(&format!(
        "postgres://oo_target_admin@127.0.0.1:{port}/{database}"
    ))
    .await
    .unwrap();
    ti::install_catalog_before_empty(&mut restricted)
        .await
        .expect("existing transaction-lock permission must suffice; do not grant a session lock");
    let after: String = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(m) ORDER BY version,phase)::text FROM _oo.schema_migrations m",
    )
    .fetch_one(&mut db)
    .await
    .unwrap();
    assert_eq!(before, after);
}
