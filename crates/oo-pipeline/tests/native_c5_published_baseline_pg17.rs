//! Published baseline API -> true single and reordered composite multipart.
//! Uses synthetic baseline receipts and the real loader_hold/fence functions.
use oo_pipeline::{native_c5_state, native_update, target_installer};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

async fn sql(db: &mut PgConnection, statement: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
        .execute(db)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires a new owned PG17 container, OO_C5_PUBLISHED_OWNED=1 and OO_C5_PUBLISHED_PG_PORT"]
async fn single_and_reordered_multipart_share_real_source_watermark() {
    assert_eq!(std::env::var("OO_C5_PUBLISHED_OWNED").unwrap(), "1");
    let port: u16 = std::env::var("OO_C5_PUBLISHED_PG_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let connect = |database: &str| format!("postgres://postgres@127.0.0.1:{port}/{database}");
    let mut admin = PgConnection::connect(&connect("postgres")).await.unwrap();
    let version: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert!(version.starts_with("17."));
    sql(&mut admin, "CREATE DATABASE c5_published_owned").await;
    let mut db = PgConnection::connect(&connect("c5_published_owned"))
        .await
        .unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut migrations = std::fs::read_dir(root.join("migrations"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "sql")
                && path.file_name().unwrap().to_str().unwrap()[..4]
                    .parse::<u32>()
                    .unwrap()
                    <= 69
        })
        .collect::<Vec<_>>();
    migrations.sort();
    for path in migrations {
        let name = path.file_name().unwrap().to_str().unwrap();
        let role = if name.starts_with("0000") || name.starts_with("0044") {
            ""
        } else {
            "SET LOCAL ROLE oo_target_owner;"
        };
        sql(
            &mut db,
            &format!(
                "BEGIN; {role}\n{}\nCOMMIT;",
                std::fs::read_to_string(&path).unwrap()
            ),
        )
        .await;
        if name.starts_with("0000") {
            sql(&mut db, "CREATE ROLE oo_jobs_owner NOLOGIN; CREATE ROLE oo_jobs_runtime NOLOGIN; CREATE ROLE oo_target_admin NOLOGIN;
                GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE;
                CREATE SCHEMA jobs AUTHORIZATION oo_jobs_owner;
                CREATE TABLE jobs.tasks(id uuid PRIMARY KEY,state text,generation bigint,lease_until timestamptz,project_id uuid,lease_owner text);
                ALTER TABLE jobs.tasks OWNER TO oo_jobs_owner;
                GRANT USAGE ON SCHEMA jobs TO oo_target_owner,oo_pipeline_runtime,oo_jobs_runtime;
                GRANT SELECT ON jobs.tasks TO oo_target_owner;").await;
        }
    }
    sql(
        &mut db,
        include_str!("support/c5_published_baseline_seed.sql"),
    )
    .await;
    sql(
        &mut db,
        &format!(
            "BEGIN; SET LOCAL ROLE oo_target_owner; {}\nCOMMIT;",
            include_str!("../migrations/0074_native_c5_resumed_baseline.sql")
        ),
    )
    .await;
    sql(
        &mut db,
        &format!(
            "SET ROLE oo_jobs_owner; {}\nRESET ROLE;",
            std::fs::read_to_string(root.join("../oo-jobs/migrations/0004_loader_hold_expand.sql"))
                .unwrap()
        ),
    )
    .await;
    sql(&mut db, "CREATE ROLE fixture69_runtime LOGIN INHERIT IN ROLE oo_pipeline_runtime,oo_jobs_runtime;
        GRANT CONNECT ON DATABASE c5_published_owned TO fixture69_runtime;
        INSERT INTO jobs.tasks VALUES('33333333-3333-4333-8333-333333333333','running',1,clock_timestamp()+interval '1 hour',
            '44444444-4444-4444-8444-444444444444','fixture');").await;
    for (version, file) in [
        (
            "native-c5-multipart-initialize/v1",
            "0065_native_c5_multipart_initialize.sql",
        ),
        (
            "native-c5-published-baseline/v1",
            "0069_native_c5_published_baseline.sql",
        ),
    ] {
        let bytes = std::fs::read(root.join("migrations").join(file)).unwrap();
        sqlx::query("INSERT INTO _oo.schema_migrations(version,checksum,phase,filename) VALUES($1,$2,'expand',$3)")
            .bind(version).bind(target_installer::sha256_hex(&bytes)).bind(file).execute(&mut db).await.unwrap();
    }
    let dir = std::env::temp_dir().join(format!("c5-published-native-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let password = dir.join("password");
    std::fs::write(&password, "").unwrap();
    std::fs::set_permissions(&password, std::fs::Permissions::from_mode(0o600)).unwrap();
    let target = json!({"host":"127.0.0.1","port":port,"database":"c5_published_owned","runtime_principal":"fixture69_runtime",
        "password_file":password,"ssl":"disable","shard":"shared_published","owner":"11111111-1111-4111-8111-111111111111"});
    let base = json!({"protocol":"oo.pipeline.native-c5-state/v1","source_id":"shared_published","schema":"app",
        "target":target,"lease_id":"33333333-3333-4333-8333-333333333333","lease_generation":1,"expected_source_generation":7});
    use native_update::{FixtureCell as Cell, FixtureColumn as Column, FixtureKind as Kind};
    for (table, multipart, empty, baseline_generation) in [
        ("single_old", false, false, 3),
        ("composite", true, false, 7),
        ("single_empty", false, true, 1),
    ] {
        let columns = if multipart {
            vec![
                Column {
                    name: "b".into(),
                    nullable: false,
                    kind: Kind::Int64,
                },
                Column {
                    name: "a".into(),
                    nullable: false,
                    kind: Kind::Int64,
                },
                Column {
                    name: "value".into(),
                    nullable: true,
                    kind: Kind::Utf8,
                },
            ]
        } else {
            vec![
                Column {
                    name: "id".into(),
                    nullable: false,
                    kind: Kind::Int64,
                },
                Column {
                    name: "value".into(),
                    nullable: true,
                    kind: Kind::Utf8,
                },
            ]
        };
        let raw: Option<String> = sqlx::query_scalar(
            "SELECT multipart_scope::text FROM _oo.full_import_run WHERE run_id=md5($1)::uuid",
        )
        .bind(table)
        .fetch_one(&mut db)
        .await
        .unwrap();
        let mut manifest: Value = raw
            .map(|r| serde_json::from_str(&r).unwrap())
            .unwrap_or(Value::Null);
        for part in 0..if multipart { 3 } else { 1 } {
            let rows = (part * 2 + 1..=part * 2 + 2)
                .filter(|_| !empty)
                .map(|a| {
                    if multipart {
                        let b = if a % 2 == 0 {
                            (a - 1) * 10
                        } else {
                            (a + 1) * 10
                        };
                        vec![Cell::Int(b), Cell::Int(a), Cell::Text(format!("v{a}"))]
                    } else {
                        vec![Cell::Int(a), Cell::Text(format!("v{a}"))]
                    }
                })
                .collect::<Vec<_>>();
            let bytes = if empty {
                native_update::encode_empty_fixture_arrow(&columns).unwrap()
            } else {
                native_update::encode_fixture_arrow(&columns, &rows).unwrap()
            };
            let file = dir.join(format!("{table}-{part}.arrow"));
            std::fs::write(&file, &bytes).unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            let sha = target_installer::sha256_hex(&bytes);
            sql(&mut db, "SET session_replication_role=replica").await;
            if multipart {
                manifest["parts"][part as usize]["arrow_sha256"] = json!(sha);
                manifest["chunks"][part as usize]["arrow_sha256"] = json!(sha);
                manifest["chunks"][part as usize]["bytes"] = json!(bytes.len());
                sqlx::query(
                    "UPDATE _oo.apply_receipts SET payload_digest=$1 WHERE txn_or_batch_id=$2",
                )
                .bind(&sha)
                .bind(format!("{table}.part.{part}"))
                .execute(&mut db)
                .await
                .unwrap();
                sqlx::query(
                    "UPDATE _oo.full_import_quality SET payload_digest=$1 WHERE batch_id=$2",
                )
                .bind(&sha)
                .bind(format!("{table}.part.{part}"))
                .execute(&mut db)
                .await
                .unwrap();
            } else {
                // Single input_digest is the raw table Arrow. The apply payload
                // digest is a different, real receipt identity and stays unchanged.
                sqlx::query(
                    "UPDATE _oo.full_import_run SET input_digest=$1 WHERE run_id=md5($2)::uuid",
                )
                .bind(&sha)
                .bind(table)
                .execute(&mut db)
                .await
                .unwrap();
                sqlx::query(
                    "UPDATE _oo.full_import_quality SET input_digest=$1 WHERE run_id=md5($2)::uuid",
                )
                .bind(&sha)
                .bind(table)
                .execute(&mut db)
                .await
                .unwrap();
            }
            sql(&mut db, "SET session_replication_role=origin").await;
        }
        if multipart {
            manifest["manifest_sha256"] = json!(target_installer::sha256_hex(
                &serde_json::to_vec(&manifest["chunks"]).unwrap()
            ));
            sql(&mut db, "SET session_replication_role=replica").await;
            sqlx::query("UPDATE _oo.full_import_run SET multipart_scope=$1::jsonb WHERE run_id=md5($2)::uuid")
                .bind(manifest.to_string()).bind(table).execute(&mut db).await.unwrap();
            sql(&mut db, "SET session_replication_role=origin").await;
        }
        let mut initialize = base.clone();
        initialize["table"] = json!(table);
        initialize["operation"] = json!("initialize_published_if_absent");
        initialize["schema_fingerprint"] = json!("a".repeat(64));
        initialize["chunk_topology_revision"] = json!(1);
        initialize["key_columns"] = if multipart {
            json!(["a", "b"])
        } else {
            json!(["id"])
        };
        if multipart {
            sql(&mut db, "BEGIN; SET LOCAL session_replication_role=replica;
                UPDATE _oo.full_import_run SET target_generation=2 WHERE run_id=md5('composite')::uuid;
                UPDATE _oo.table_freshness SET target_generation=2 WHERE table_name='composite';
                UPDATE _oo.update_batch SET target_generation=2 WHERE run_id=md5('composite')::uuid;
                UPDATE _oo.apply_receipts SET fence_generation=2 WHERE txn_or_batch_id IN
                    ('composite.part.1','composite.part.2','composite.final');
                COMMIT;").await;
            for invalid_fence in [3] {
                sql(
                    &mut db,
                    &format!(
                        "BEGIN; SET LOCAL session_replication_role=replica;
                    UPDATE _oo.apply_receipts SET fence_generation={invalid_fence}
                    WHERE txn_or_batch_id='composite.part.0'; COMMIT;"
                    ),
                )
                .await;
                assert!(native_c5_state::execute(&initialize).await.is_err());
            }
            sql(
                &mut db,
                "BEGIN; SET LOCAL session_replication_role=replica;
                UPDATE _oo.apply_receipts SET fence_generation=1,fence_shard='foreign_shard'
                    WHERE txn_or_batch_id='composite.part.0';
                COMMIT;",
            )
            .await;
            assert!(native_c5_state::execute(&initialize).await.is_err());
            sql(
                &mut db,
                "BEGIN; SET LOCAL session_replication_role=replica;
                UPDATE _oo.apply_receipts SET fence_shard='shared_published'
                    WHERE txn_or_batch_id='composite.part.0'; COMMIT;",
            )
            .await;
        }
        let result = native_c5_state::execute(&initialize)
            .await
            .expect("published initializer");
        assert_eq!(
            result["baseline_kind"],
            if multipart { "multipart" } else { "single" }
        );
        assert_eq!(result["source_generation"], 7);
        assert_eq!(result["table_source_generation"], baseline_generation);
        assert_eq!(
            native_c5_state::execute(&initialize).await.unwrap()["created"],
            false
        );
        let mut hydration = base.clone();
        hydration["table"] = json!(table);
        hydration["operation"] = json!("hydrate_multipart");
        if multipart {
            let mut tx = db.begin().await.unwrap();
            let fence:i64=sqlx::query_scalar("SELECT _oo.adopt_target_generation('shared_published','11111111-1111-4111-8111-111111111111')")
                .fetch_one(&mut *tx).await.unwrap();
            sqlx::query("SELECT _oo.full_import_bind_session('shared_published','11111111-1111-4111-8111-111111111111',$1)")
                .bind(fence).execute(&mut *tx).await.unwrap();
            let wrong_members = json!([[{"kind":"int","value":20},{"kind":"int","value":1}],
                [{"kind":"int","value":10},{"kind":"int","value":2}]]);
            let error=sqlx::query("SELECT _oo.c5_hydrate_multipart('shared_published','11111111-1111-4111-8111-111111111111',$1,
                'shared_published','app','composite','0',7,$2,$3::jsonb)")
                .bind(fence).bind(manifest["parts"][0]["arrow_sha256"].as_str().unwrap())
                .bind(wrong_members.to_string()).execute(&mut *tx).await.unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("retained source key order differs")
            );
            tx.rollback().await.unwrap();
            sql(
                &mut db,
                "SELECT _oo.native_update_release_fence('shared_published')",
            )
            .await;
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT count(*) FROM _oo.c5_member WHERE table_name='composite'"
                )
                .fetch_one(&mut db)
                .await
                .unwrap(),
                0
            );
        } else if !empty {
            hydration["chunk_id"] = json!("0");
            hydration["arrow_path"] = json!(dir.join("single_old-0.arrow"));
            let original = std::fs::read(dir.join("single_old-0.arrow")).unwrap();
            std::fs::write(dir.join("single_old-0.arrow"), b"not retained Arrow").unwrap();
            assert_eq!(
                native_c5_state::execute(&hydration).await,
                Err("c5_hydration_digest")
            );
            std::fs::write(dir.join("single_old-0.arrow"), original).unwrap();
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT count(*) FROM _oo.c5_member WHERE table_name='single_old'"
                )
                .fetch_one(&mut db)
                .await
                .unwrap(),
                0
            );
        }
        for part in 0..if multipart { 3 } else { 1 } {
            hydration["chunk_id"] = json!(part.to_string());
            hydration["arrow_path"] = json!(dir.join(format!("{table}-{part}.arrow")));
            let result = native_c5_state::execute(&hydration)
                .await
                .expect("retained hydration");
            assert_eq!(result["hydrated"], true);
            assert_eq!(result["archive_digest_seeded"], true);
            assert_eq!(
                native_c5_state::execute(&hydration).await.unwrap()["replay"],
                true
            );
        }
        let mut export = base.clone();
        export["table"] = json!(table);
        export["operation"] = json!("export");
        let authority = native_c5_state::execute(&export).await.unwrap();
        assert_eq!(
            authority["baseline_kind"],
            if multipart { "multipart" } else { "single" }
        );
        assert_eq!(authority["source_generation"], 7);
        assert_eq!(authority["table_source_generation"], baseline_generation);
        assert_eq!(
            authority["selected_columns"],
            if multipart {
                json!(["b", "a", "value"])
            } else {
                json!(["id", "value"])
            }
        );
        assert_eq!(
            authority["baseline"]["key_columns"],
            initialize["key_columns"]
        );
        assert_eq!(authority["type_policy_version"], "type-policy/v1");
        for chunk in authority["chunks"].as_array().unwrap() {
            assert_eq!(chunk["baseline_membership_pending"], false);
            assert_eq!(chunk["digest_codec"], "oo.c5.arrow-sha256-xor/v1");
            assert!(chunk["last_digest_scan_id"].is_null());
            if empty {
                assert_eq!(
                    chunk["digest"],
                    json!([0, "0000000000000000", "0000000000000000"])
                );
            }
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let expected = authority["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["chunk_id"].clone())
            .collect::<Vec<_>>();
        let mut scan = base.clone();
        scan["table"] = json!(table);
        scan["operation"] = json!("begin_scan");
        scan["scan_id"] = json!(format!("published-scan:{table}"));
        scan["expected_chunks"] = json!(expected);
        scan["scan_layers"] = json!([false, false, true, false]);
        scan["scan_observed_at_ms"] = json!(now);
        assert_eq!(
            native_c5_state::execute(&scan).await.unwrap()["next_chunk_index"],
            0
        );
        let mut observe = base.clone();
        observe["table"] = json!(table);
        observe["operation"] = json!("observe");
        observe["nochange"] = json!({"protocol":"oo.jobs.native-c5-nochange/v1","source_id":"shared_published",
            "source_generation":7,"acknowledged_source_generation":7,"data_gen":false,"apply":false,"ack":false,
            "c5_observation":{"scan_id":scan["scan_id"],"expected_chunks":expected,"schema_fingerprint":"a".repeat(64),
                "chunk_topology_revision":1,"checked_at_ms":now,"chunks":authority["chunks"],"full_digest_scan_completed":true}});
        assert_eq!(
            native_c5_state::execute(&observe).await.unwrap()["data_gen"],
            false
        );
        assert_eq!(
            native_c5_state::execute(&export).await.unwrap()["table_source_generation"],
            baseline_generation
        );
        assert!(sqlx::query_scalar::<_,bool>("SELECT last_full_digest_scan_completed_at IS NOT NULL FROM _oo.c5_table WHERE table_name=$1")
            .bind(table).fetch_one(&mut db).await.unwrap());
    }
    let single_null:bool=sqlx::query_scalar("SELECT multipart_scope IS NULL FROM _oo.full_import_run WHERE run_id=md5('single_old')::uuid")
        .fetch_one(&mut db).await.unwrap();
    assert!(
        single_null,
        "a single baseline never acquires a synthetic manifest"
    );
    let members:Vec<(String,String)>=sqlx::query_as("SELECT chunk_id,typed_key::text FROM _oo.c5_member WHERE table_name='composite' ORDER BY chunk_id,key_identity")
        .fetch_all(&mut db).await.unwrap();
    assert_eq!(members.len(), 6);
    assert_eq!(
        serde_json::from_str::<Value>(&members[0].1).unwrap(),
        json!([{"kind":"int","value":1},{"kind":"int","value":20}])
    );
    let generations: Vec<(String, i64)> =
        sqlx::query_as("SELECT table_name,source_generation FROM _oo.c5_table ORDER BY table_name")
            .fetch_all(&mut db)
            .await
            .unwrap();
    assert_eq!(
        generations,
        vec![
            ("composite".into(), 7),
            ("single_empty".into(), 1),
            ("single_old".into(), 3)
        ]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT source_generation FROM _oo.source_watermark")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        7
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM _oo.apply_receipts")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        6
    );
    for fault in [
        "DELETE FROM _oo.apply_receipts WHERE table_generation=(SELECT physical_name FROM _oo.full_import_table WHERE run_id=md5('single_old')::uuid)",
        "UPDATE _oo.full_import_quality SET accepted_rows=3 WHERE run_id=md5('single_old')::uuid",
    ] {
        let mut tx = db.begin().await.unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(
            "SET LOCAL session_replication_role=replica",
        ))
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(fault))
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query("SELECT _oo.c5_single_baseline_receipt(md5('single_old')::uuid)")
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("sealed single baseline receipt differs")
        );
        tx.rollback().await.unwrap();
    }
    for name in [
        "password",
        "single_old-0.arrow",
        "single_empty-0.arrow",
        "composite-0.arrow",
        "composite-1.arrow",
        "composite-2.arrow",
    ] {
        std::fs::remove_file(dir.join(name)).unwrap();
    }
    std::fs::remove_dir(dir).unwrap();
    println!(
        "PASS: shared source H=7, single B=3, composite PK [a,b] over physical/index [b,a], empty single, no invented manifest."
    );
}
