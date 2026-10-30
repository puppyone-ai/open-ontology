//! Empty-only API checks with synthetic sealed receipts and a real target lease.
//! Uses synthetic baseline receipts and the real loader_hold/fence functions.
use oo_pipeline::{native_c5_state, target_installer};
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
#[ignore = "requires a new owned PG17 container, OO_C5_EMPTY_OWNED=1 and OO_C5_EMPTY_PG_PORT"]
async fn empty_only_keeps_baseline_and_requires_current_source_proof() {
    assert_eq!(std::env::var("OO_C5_EMPTY_OWNED").unwrap(), "1");
    let port: u16 = std::env::var("OO_C5_EMPTY_PG_PORT")
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
    sql(&mut admin, "CREATE DATABASE c5_empty_owned").await;
    let mut db = PgConnection::connect(&connect("c5_empty_owned"))
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
                    <= 71
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
            "SET ROLE oo_jobs_owner; {}\nRESET ROLE;",
            std::fs::read_to_string(root.join("../oo-jobs/migrations/0004_loader_hold_expand.sql"))
                .unwrap()
        ),
    )
    .await;
    sql(&mut db, "CREATE ROLE fixture71_runtime LOGIN INHERIT IN ROLE oo_pipeline_runtime,oo_jobs_runtime;
        GRANT CONNECT ON DATABASE c5_empty_owned TO fixture71_runtime;
        INSERT INTO jobs.tasks VALUES('33333333-3333-4333-8333-333333333333','running',1,clock_timestamp()+interval '1 hour',
            '44444444-4444-4444-8444-444444444444','fixture');").await;
    for (version, file) in [
        ("native-c5-empty-baseline/v1", "0071_native_c5_empty_baseline.sql"),
    ] {
        let bytes = std::fs::read(root.join("migrations").join(file)).unwrap();
        sqlx::query("INSERT INTO _oo.schema_migrations(version,checksum,phase,filename) VALUES($1,$2,'expand',$3)")
            .bind(version).bind(target_installer::sha256_hex(&bytes)).bind(file).execute(&mut db).await.unwrap();
    }
    let dir = std::env::temp_dir().join(format!("c5-empty-native-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let password = dir.join("password");
    std::fs::write(&password, "").unwrap();
    std::fs::set_permissions(&password, std::fs::Permissions::from_mode(0o600)).unwrap();
    let target = json!({"host":"127.0.0.1","port":port,"database":"c5_empty_owned","runtime_principal":"fixture71_runtime",
        "password_file":password,"ssl":"disable","shard":"shared_published","owner":"11111111-1111-4111-8111-111111111111"});
    let base = json!({"protocol":"oo.pipeline.native-c5-state/v1","source_id":"shared_published","schema":"app",
        "target":target,"lease_id":"33333333-3333-4333-8333-333333333333","lease_generation":1,"expected_source_generation":7});
    sql(&mut db, "BEGIN; SET LOCAL session_replication_role=replica;
        SELECT fixture69.seed('empty_date',2,false,true);
        SELECT fixture69.seed('empty_numeric',4,false,true); COMMIT;
        DO $$ DECLARE t text; physical text; col text; BEGIN
        FOREACH t IN ARRAY ARRAY['single_empty','empty_date','empty_numeric'] LOOP
          SELECT physical_name INTO physical FROM _oo.table_freshness WHERE table_name=t;
          SELECT physical_name INTO col FROM _oo.physical_mappings
           WHERE logical_table=t AND logical_column='id' AND level='c';
          IF t='single_empty' THEN
            EXECUTE format('ALTER TABLE _oo.%I ALTER COLUMN %I DROP NOT NULL',physical,col);
          ELSE
            EXECUTE format('ALTER TABLE _oo.%I ALTER COLUMN %I TYPE %s USING NULL',
              physical,col,CASE WHEN t='empty_date' THEN 'date' ELSE 'numeric(20,0)' END);
          END IF;
        END LOOP; END $$;").await;
    let before: String = sqlx::query_scalar(
        "SELECT jsonb_build_object('receipts',(SELECT count(*) FROM _oo.apply_receipts),
         'freshness',(SELECT jsonb_agg(to_jsonb(f) ORDER BY table_name) FROM _oo.table_freshness f),
         'watermark',(SELECT to_jsonb(w) FROM _oo.source_watermark w))::text",
    ).fetch_one(&mut db).await.unwrap();
    let mut nonempty = base.clone();
    nonempty["operation"] = json!("initialize_empty_if_absent");
    nonempty["table"] = json!("single_old");
    nonempty["schema_fingerprint"] = json!("a".repeat(64));
    nonempty["chunk_topology_revision"] = json!(1);
    nonempty["key_columns"] = json!([]);
    assert!(native_c5_state::execute(&nonempty).await.is_err());
    for (table, generation, oid) in [
        ("single_empty", 1, 20),
        ("empty_date", 2, 1082),
        ("empty_numeric", 4, 1700),
    ] {
        let mut init = base.clone();
        init["operation"] = json!("initialize_empty_if_absent");
        init["table"] = json!(table);
        init["schema_fingerprint"] = json!("a".repeat(64));
        init["chunk_topology_revision"] = json!(1);
        init["key_columns"] = json!([]);
        let authority = native_c5_state::execute(&init).await.unwrap();
        assert_eq!(authority["empty_only"], true);
        assert_eq!(authority["table_source_generation"], generation);
        assert_eq!(authority["source_generation"], 7);
        assert_eq!(authority["columns"][0]["target_oid"], oid);
        assert_eq!(authority["baseline_receipt"]["row_count"], 0);
        assert_eq!(authority["chunks"], json!([]));
        assert_eq!(authority["key_columns"], json!([]));
        assert!(authority["last_empty_confirmed_at_ms"].is_null());
        let replay = native_c5_state::execute(&init).await.unwrap();
        assert_eq!(replay["created"], false);
        assert_eq!(replay["empty_observation"], authority["empty_observation"]);
        let request = confirmation(&init, &authority);
        for (field, value) in [
            ("rows_returned", json!(1)),
            ("row_limit", json!(2)),
            ("source_query", json!("SELECT 1 FROM `app`.`single_empty` WHERE 1=0 LIMIT 1")),
            ("schema_checked_after", json!(false)),
            ("snapshot_complete", json!(false)),
            ("read_only", json!(false)),
            ("observation_id", json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")),
            ("started_at_ms", json!(0)),
            ("schema_fingerprint", json!("b".repeat(64))),
            ("table_source_generation", json!(7)),
        ] {
            let mut bad = request.clone();
            bad["nochange"]["c5_observation"][field] = value;
            assert!(native_c5_state::execute(&bad).await.is_err(), "{table} {field}");
        }
        let mut bad = request.clone();
        bad["nochange"]["c5_observation"]["baseline_receipt"]["config_digest"] = json!("f".repeat(64));
        assert!(native_c5_state::execute(&bad).await.is_err());
        let mut export = init.clone();
        export["operation"] = json!("export");
        assert!(native_c5_state::execute(&export).await.unwrap()["last_empty_confirmed_at_ms"].is_null());
        let result = native_c5_state::execute(&request).await.unwrap();
        assert_eq!(result["empty_baseline_confirmed"], true);
        assert_eq!(result["replay"], false);
        for field in ["data_gen", "apply", "ack"] {
            assert_eq!(result[field], false);
        }
        let replay = native_c5_state::execute(&request).await.unwrap();
        assert_eq!(replay["replay"], true);
        assert_eq!(result["last_empty_confirmed_at_ms"], replay["last_empty_confirmed_at_ms"]);
        let mut bad = request.clone();
        bad["nochange"]["c5_observation"]["checked_at_ms"] =
            json!(request["nochange"]["c5_observation"]["checked_at_ms"].as_i64().unwrap() + 1);
        assert!(native_c5_state::execute(&bad).await.is_err());
        let exported = native_c5_state::execute(&export).await.unwrap();
        assert_eq!(exported["last_empty_confirmed_at_ms"], result["last_empty_confirmed_at_ms"]);
    }
    let after: String = sqlx::query_scalar(
        "SELECT jsonb_build_object('receipts',(SELECT count(*) FROM _oo.apply_receipts),
         'freshness',(SELECT jsonb_agg(to_jsonb(f) ORDER BY table_name) FROM _oo.table_freshness f),
         'watermark',(SELECT to_jsonb(w) FROM _oo.source_watermark w))::text",
    ).fetch_one(&mut db).await.unwrap();
    assert_eq!(before, after, "confirmation must not mutate data freshness or generations");
    let range_states: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo.c5_table").fetch_one(&mut db).await.unwrap();
    assert_eq!(range_states, 0);
    assert!(sqlx::query("INSERT INTO _oo.c5_table(source,schema_name,table_name,schema_fingerprint,topology_revision,source_generation)
        VALUES('shared_published','app','single_empty',repeat('a',64),1,1)")
        .execute(&mut db).await.is_err(), "zero-only state cannot become a synthetic-key range");

    let mut init = base.clone();
    init["operation"] = json!("initialize_empty_if_absent");
    init["table"] = json!("single_empty");
    init["schema_fingerprint"] = json!("a".repeat(64));
    init["chunk_topology_revision"] = json!(1);
    init["key_columns"] = json!([]);
    let old = native_c5_state::execute(&init).await.unwrap();
    sql(&mut db, "INSERT INTO jobs.tasks VALUES('55555555-5555-4555-8555-555555555555','running',1,
        clock_timestamp()+interval '1 hour','44444444-4444-4444-8444-444444444444','fixture');
        UPDATE _oo.source_watermark SET source_generation=8 WHERE source='shared_published';").await;
    init["expected_source_generation"] = json!(8);
    init["lease_id"] = json!("55555555-5555-4555-8555-555555555555");
    assert!(native_c5_state::execute(&init).await.is_err(), "cannot replace another live lease's context");
    sql(&mut db, "UPDATE jobs.tasks SET state='succeeded' WHERE id='33333333-3333-4333-8333-333333333333';").await;
    let new = native_c5_state::execute(&init).await.unwrap();
    assert_ne!(new["empty_observation"]["id"], old["empty_observation"]["id"]);
    assert_eq!(new["source_generation"], 8);
    assert_eq!(new["table_source_generation"], 1);
    assert_eq!(new["last_empty_confirmed_at_ms"], old["last_empty_confirmed_at_ms"]);
    let mut stale = confirmation(&init, &new);
    stale["nochange"]["c5_observation"]["observation_id"] = old["empty_observation"]["id"].clone();
    assert!(native_c5_state::execute(&stale).await.is_err());
    assert_eq!(native_c5_state::execute(&confirmation(&init, &new)).await.unwrap()["empty_baseline_confirmed"], true);
    sql(&mut db, "UPDATE jobs.tasks SET lease_until=clock_timestamp()-interval '1 second'
        WHERE id='55555555-5555-4555-8555-555555555555';").await;
    assert!(native_c5_state::execute(&confirmation(&init, &new)).await.is_err());
    std::fs::remove_file(password).unwrap();
    std::fs::remove_dir(dir).unwrap();
    println!("PASS: keyed-unsupported/keyless empty proof, B<H, context/replay/lease guards, no range or data mutation.");
}

fn confirmation(init: &Value, authority: &Value) -> Value {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let mut request = init.clone();
    request["operation"] = json!("observe");
    request["nochange"] = json!({
        "protocol":"oo.jobs.native-c5-nochange/v1","source_id":init["source_id"],
        "method":"C5","capture_method":"c5_diff","empty_diff":true,"data_gen":false,"apply":false,"ack":false,
        "source_generation":authority["source_generation"],"acknowledged_source_generation":authority["source_generation"],
        "parent_digest":authority["input_digest"],"source_digest":"9".repeat(64),
        "snapshot":{"kind":"c5_cycle","value":"9".repeat(64)},
        "c5_observation":{
            "protocol":"oo.c5.empty-observation/v1","source_id":init["source_id"],"schema":init["schema"],"table":init["table"],
            "source_generation":authority["source_generation"],"table_source_generation":authority["table_source_generation"],
            "schema_fingerprint":authority["schema_fingerprint"],"baseline_receipt":authority["baseline_receipt"],
            "observation_id":authority["empty_observation"]["id"],"lease_id":init["lease_id"],"lease_generation":init["lease_generation"],
            "started_at_ms":now,"checked_at_ms":now,
            "source_query":format!("SELECT 1 FROM `app`.`{}` LIMIT 1",init["table"].as_str().unwrap()),
            "rows_returned":0,"row_limit":1,"snapshot_complete":true,"read_only":true,
            "schema_checked_before":true,"schema_checked_after":true
        }
    });
    request
}
