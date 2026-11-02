//! Native state API -> retained Arrow decoder -> real PG membership.
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
#[ignore = "requires a new owned PG17 container, OO_C5_INITIALIZER_OWNED=1 and OO_C5_INITIALIZER_PG_PORT"]
async fn retained_parts_hydrate_through_native_state_without_source_access() {
    assert_eq!(std::env::var("OO_C5_INITIALIZER_OWNED").unwrap(), "1");
    let port: u16 = std::env::var("OO_C5_INITIALIZER_PG_PORT")
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
    sql(&mut admin, "CREATE DATABASE c5_initializer_owned").await;
    let mut db = PgConnection::connect(&connect("c5_initializer_owned"))
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
                    <= 65
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
    let seed = include_str!("support/c5_multipart_initialize_fixture.sql")
        .split("-- END SYNTHETIC BASELINE SETUP")
        .next()
        .unwrap();
    sql(&mut db, &format!("{seed}\nCOMMIT;")).await;
    sql(
        &mut db,
        &format!(
            "SET ROLE oo_jobs_owner; {}\nRESET ROLE;",
            std::fs::read_to_string(root.join("../oo-jobs/migrations/0004_loader_hold_expand.sql"))
                .unwrap()
        ),
    )
    .await;
    sql(&mut db, "CREATE ROLE fixture_runtime LOGIN INHERIT IN ROLE oo_pipeline_runtime,oo_jobs_runtime;
        GRANT CONNECT ON DATABASE c5_initializer_owned TO fixture_runtime;
        INSERT INTO jobs.tasks VALUES('33333333-3333-4333-8333-333333333333','running',1,clock_timestamp()+interval '1 hour',
            '44444444-4444-4444-8444-444444444444','fixture');").await;
    let migration = include_str!("../migrations/0065_native_c5_multipart_initialize.sql");
    sqlx::query("INSERT INTO _oo.schema_migrations(version,checksum,phase,filename) VALUES($1,$2,'expand',$3)")
        .bind("native-c5-multipart-initialize/v1")
        .bind(target_installer::sha256_hex(migration.as_bytes()))
        .bind("0065_native_c5_multipart_initialize.sql")
        .execute(&mut db).await.unwrap();

    let directory =
        std::env::temp_dir().join(format!("c5-hydration-native-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let password = directory.join("password");
    std::fs::write(&password, "").unwrap();
    std::fs::set_permissions(&password, std::fs::Permissions::from_mode(0o600)).unwrap();
    let target = json!({"host":"127.0.0.1","port":port,"database":"c5_initializer_owned",
        "runtime_principal":"fixture_runtime","password_file":password,"ssl":"disable",
        "shard":"shared","owner":"11111111-1111-4111-8111-111111111111"});
    let base = json!({"protocol":"oo.pipeline.native-c5-state/v1","source_id":"shared","schema":"app","table":"older",
        "target":target,"lease_id":"33333333-3333-4333-8333-333333333333","lease_generation":1,
        "expected_source_generation":7});
    let raw: String = sqlx::query_scalar("SELECT multipart_scope::text FROM _oo.full_import_run")
        .fetch_one(&mut db)
        .await
        .unwrap();
    let mut manifest: Value = serde_json::from_str(&raw).unwrap();
    let columns = [
        native_update::FixtureColumn {
            name: "id".into(),
            nullable: false,
            kind: native_update::FixtureKind::Int64,
        },
        native_update::FixtureColumn {
            name: "value".into(),
            nullable: true,
            kind: native_update::FixtureKind::Utf8,
        },
    ];
    for index in 0..3 {
        let rows = (index * 2 + 1..=index * 2 + 2)
            .map(|id| {
                vec![
                    native_update::FixtureCell::Int(id),
                    native_update::FixtureCell::Text(format!("value-{id}")),
                ]
            })
            .collect::<Vec<_>>();
        let bytes = native_update::encode_fixture_arrow(&columns, &rows).unwrap();
        let path = directory.join(format!("{index}.arrow"));
        std::fs::write(&path, &bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let sha = target_installer::sha256_hex(&bytes);
        manifest["parts"][index as usize]["arrow_sha256"] = json!(sha);
        manifest["chunks"][index as usize]["arrow_sha256"] = json!(sha);
        manifest["chunks"][index as usize]["bytes"] = json!(bytes.len());
        sql(&mut db, "SET session_replication_role=replica").await;
        sqlx::query("UPDATE _oo.apply_receipts SET payload_digest=$1 WHERE txn_or_batch_id=$2")
            .bind(&sha)
            .bind(format!("fixture.part.{index}"))
            .execute(&mut db)
            .await
            .unwrap();
        sqlx::query("UPDATE _oo.full_import_quality SET payload_digest=$1 WHERE batch_id=$2")
            .bind(&sha)
            .bind(format!("fixture.part.{index}"))
            .execute(&mut db)
            .await
            .unwrap();
        sql(&mut db, "SET session_replication_role=origin").await;
    }
    manifest["manifest_sha256"] = json!(target_installer::sha256_hex(
        &serde_json::to_vec(&manifest["chunks"]).unwrap()
    ));
    sql(&mut db, "SET session_replication_role=replica").await;
    sqlx::query("UPDATE _oo.full_import_run SET multipart_scope=$1::jsonb")
        .bind(manifest.to_string())
        .execute(&mut db)
        .await
        .unwrap();
    sql(&mut db, "SET session_replication_role=origin").await;

    let mut initialize = base.clone();
    initialize["operation"] = json!("initialize_multipart_if_absent");
    initialize["schema_fingerprint"] = json!("a".repeat(64));
    initialize["chunk_topology_revision"] = json!(1);
    initialize["key_columns"] = json!(["id"]);
    let result = native_c5_state::execute(&initialize).await.unwrap();
    assert_eq!(result["table_source_generation"], 3);
    assert_eq!(result["source_generation"], 7);
    let mut hydration = base.clone();
    hydration["operation"] = json!("hydrate_multipart");
    for index in 0..3 {
        hydration["chunk_id"] = json!(index.to_string());
        hydration["arrow_path"] = json!(directory.join(format!("{index}.arrow")));
        assert_eq!(
            native_c5_state::execute(&hydration).await.unwrap()["hydrated"],
            true
        );
        assert_eq!(
            native_c5_state::execute(&hydration).await.unwrap()["replay"],
            true
        );
    }
    let members: Vec<(String, String)> =
        sqlx::query_as("SELECT chunk_id,loader_key FROM _oo.c5_member ORDER BY loader_key")
            .fetch_all(&mut db)
            .await
            .unwrap();
    assert_eq!(
        members,
        vec![
            ("0".into(), "1".into()),
            ("0".into(), "2".into()),
            ("1".into(), "3".into()),
            ("1".into(), "4".into()),
            ("2".into(), "5".into()),
            ("2".into(), "6".into())
        ]
    );
    let generations: (i64,i64,i64) = sqlx::query_as(
        "SELECT (SELECT source_generation FROM _oo.c5_table),(SELECT source_generation FROM _oo.source_watermark),(SELECT count(*) FROM _oo.apply_receipts)")
        .fetch_one(&mut db).await.unwrap();
    assert_eq!(generations, (3, 7, 4));
    let mut export = base.clone();
    export["operation"] = json!("export");
    let seeded = native_c5_state::execute(&export).await.unwrap();
    assert_eq!(seeded["selected_columns"], json!(["id", "value"]));
    for field in [
        "scan_id",
        "expected_chunks",
        "scan_layers",
        "scan_observed_at_ms",
    ] {
        assert_eq!(seeded[field], Value::Null, "{field}");
    }
    assert_eq!(seeded["next_chunk_index"], 0);
    for chunk in seeded["chunks"].as_array().unwrap() {
        assert_eq!(chunk["digest_codec"], "oo.c5.arrow-sha256-xor/v1");
        assert_eq!(chunk["digest"][0], 2);
        assert!(chunk["last_digest_scan_id"].is_null());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut begin = base.clone();
    begin["operation"] = json!("begin_scan");
    begin["scan_id"] = json!("native-scan");
    begin["expected_chunks"] = json!(["0", "1", "2"]);
    begin["scan_layers"] = json!([false, false, true, false]);
    begin["scan_observed_at_ms"] = json!(now);
    assert_eq!(
        native_c5_state::execute(&begin).await.unwrap()["next_chunk_index"],
        0
    );
    let active = native_c5_state::execute(&export).await.unwrap();
    for field in [
        "scan_id",
        "expected_chunks",
        "scan_layers",
        "scan_observed_at_ms",
    ] {
        assert_eq!(active[field], begin[field], "{field}");
    }
    let mut different = begin.clone();
    different["scan_observed_at_ms"] = json!(now + 1);
    assert_eq!(
        native_c5_state::execute(&different).await,
        Err("c5_scan_context")
    );
    let mut partial = base.clone();
    partial["operation"] = json!("observe_partial");
    partial["observation"] = json!({"scan_id":"native-scan","expected_chunks":["0","1","2"],
        "schema_fingerprint":"a".repeat(64),"chunk_topology_revision":1,"checked_at_ms":now,
        "chunks":[active["chunks"][0],active["chunks"][1]],"chunk":active["chunks"][1]});
    assert_eq!(
        native_c5_state::execute(&partial).await.unwrap()["next_chunk_index"],
        2
    );
    assert_eq!(
        native_c5_state::execute(&begin).await.unwrap()["next_chunk_index"],
        2
    );
    let mut unordered = partial.clone();
    unordered["observation"]["chunks"] = json!([active["chunks"][1], active["chunks"][0]]);
    assert_eq!(
        native_c5_state::execute(&unordered).await,
        Err("c5_partial_observation")
    );
    let mut nochange = base.clone();
    nochange["operation"] = json!("observe");
    nochange["nochange"] = json!({"protocol":"oo.jobs.native-c5-nochange/v1","source_id":"shared",
        "source_generation":7,"acknowledged_source_generation":7,"data_gen":false,"apply":false,"ack":false,
        "c5_observation":{"scan_id":"native-scan","expected_chunks":["0","1","2"],
            "schema_fingerprint":"a".repeat(64),"chunk_topology_revision":1,"checked_at_ms":now,
            "chunks":active["chunks"],"full_digest_scan_completed":true}});
    assert_eq!(
        native_c5_state::execute(&nochange).await.unwrap()["data_gen"],
        false
    );
    let completed = native_c5_state::execute(&export).await.unwrap();
    assert_eq!(completed["source_generation"], 7);
    assert_eq!(completed["table_source_generation"], 3);
    assert_eq!(completed["next_chunk_index"], 0);
    for field in [
        "scan_id",
        "expected_chunks",
        "scan_layers",
        "scan_observed_at_ms",
    ] {
        assert_eq!(completed[field], Value::Null, "{field}");
    }
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT last_full_digest_scan_completed_at IS NOT NULL FROM _oo.c5_table"
        )
        .fetch_one(&mut db)
        .await
        .unwrap()
    );
    // Recent-window/no-active observations do not invent a full scan or generation.
    nochange["nochange"]["c5_observation"]["scan_id"] = Value::Null;
    nochange["nochange"]["c5_observation"]["expected_chunks"] = Value::Null;
    nochange["nochange"]["c5_observation"]["full_digest_scan_completed"] = json!(false);
    assert_eq!(
        native_c5_state::execute(&nochange).await.unwrap()["apply"],
        false
    );
    std::fs::write(directory.join("2.arrow"), b"corrupt retained bytes").unwrap();
    assert_eq!(
        native_c5_state::execute(&hydration).await,
        Err("multipart_digest")
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo.c5_member")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(count, 6);
    for name in ["password", "0.arrow", "1.arrow", "2.arrow"] {
        std::fs::remove_file(directory.join(name)).unwrap();
    }
    std::fs::remove_dir(directory).unwrap();
}
