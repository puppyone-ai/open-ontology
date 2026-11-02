//! Real MySQL 5.7 ROW/FULL/GTID -> Arrow -> PostgreSQL private typed stage.
//! Uses only the named isolated fixture. Not source-onboarding/publication acceptance.
use mysql_async::{Conn, OptsBuilder, prelude::Queryable};
use mysql_preview::c1_typed_source::Capture;
use oo_pipeline::{
    c1_typed::{self, Batch, Lease, Receipt},
    target_installer as ti,
};
use serde_json::{Value, json};
use sqlx::{
    Connection, PgConnection,
    postgres::{PgConnectOptions, PgSslMode},
};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[path = "fixtures/c1_typed_owned/install.rs"]
mod migrations;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn home() -> PathBuf {
    PathBuf::from(std::env::var("OO_C1_TYPED_HOME").expect("explicit owned fixture opt-in"))
}
fn secret(name: &str) -> String {
    fs::read_to_string(home().join(format!("secrets/{name}.pw")))
        .unwrap()
        .trim()
        .into()
}
async fn mysql() -> Result<Conn> {
    Ok(Conn::new(
        OptsBuilder::default()
            .ip_or_hostname("127.0.0.1")
            .tcp_port(23327)
            .user(Some("root"))
            .pass(Some(secret("mysql-root")))
            .prefer_socket(false),
    )
    .await?)
}

#[tokio::test]
#[ignore = "isolated fixture TLS diagnostic; never disables certificate verification"]
async fn owned_tls_probe() -> Result<()> {
    let password = secret("cdc");
    let opts = OptsBuilder::default()
        .ip_or_hostname("127.0.0.1")
        .tcp_port(23327)
        .user(Some("oo_cdc"))
        .pass(Some(password.clone()))
        .db_name(Some("c1_typed"))
        .prefer_socket(false)
        .ssl_opts(
            mysql_async::SslOpts::default()
                .with_root_certs(vec![home().join("certs/ca.pem").into()])
                .with_disable_built_in_roots(true)
                .with_danger_tls_hostname_override(Some("localhost")),
        );
    match Conn::new(opts).await {
        Ok(mut conn) => {
            let n: Option<u64> = conn.query_first("SELECT 1").await?;
            assert_eq!(n, Some(1));
            Ok(())
        }
        Err(e) => Err(format!(
            "owned TLS diagnostic: {}",
            e.to_string().replace(&password, "[redacted]")
        )
        .into()),
    }
}
async fn pg(db: &str) -> Result<PgConnection> {
    Ok(PgConnection::connect_with(
        &PgConnectOptions::new()
            .host("127.0.0.1")
            .port(25487)
            .username("postgres")
            .password(&secret("pg-super"))
            .database(db)
            .ssl_mode(PgSslMode::Disable),
    )
    .await?)
}
async fn sql(c: &mut PgConnection, s: &str) -> Result<()> {
    sqlx::raw_sql(sqlx::AssertSqlSafe(s.to_string()))
        .execute(c)
        .await?;
    Ok(())
}
async fn ack(c: &mut PgConnection, l: &Lease, r: &Receipt) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT _oo.c1_typed_ack($1,$2::uuid,$3,$4,$5,$6)")
            .bind(&l.source)
            .bind(&l.task)
            .bind(l.generation)
            .bind(l.epoch)
            .bind(r.sequence)
            .bind(&r.digest)
            .fetch_one(c)
            .await?,
    )
}
async fn apply_ack(c: &mut PgConnection, l: &Lease, b: &Batch) -> Result<Receipt> {
    let r = c1_typed::apply(c, l, b).await?;
    assert!(ack(c, l, &r).await?);
    Ok(r)
}

async fn setup_target(db: &str, cap: &Capture) -> Result<(PgConnection, Lease)> {
    let mut admin = pg("postgres").await?;
    sql(&mut admin, &format!("CREATE DATABASE {db}")).await?;
    let mut c = pg(db).await?;
    ti::install(&mut c).await?;
    let roles: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_jobs_owner')")
            .fetch_one(&mut c)
            .await?;
    if !roles {
        sql(
            &mut c,
            include_str!("../../../crates/oo-jobs/migrations/0000_bootstrap_roles.sql"),
        )
        .await?;
    } else {
        sql(
            &mut c,
            &format!("GRANT CONNECT,CREATE ON DATABASE {db} TO oo_jobs_owner"),
        )
        .await?;
    }
    sql(&mut c, "SET ROLE oo_jobs_owner").await?;
    sql(
        &mut c,
        include_str!("../../../crates/oo-jobs/migrations/0001_jobs_control_expand.sql"),
    )
    .await?;
    sql(&mut c,"RESET ROLE; GRANT USAGE ON SCHEMA jobs TO oo_target_owner,oo_pipeline_runtime; GRANT SELECT ON jobs.tasks TO oo_target_owner").await?;
    migrations::install(&mut c).await;
    ti::install_native_c1_typed_keys(&mut c).await?; // exact migration replay
    let task: String = sqlx::query_scalar("SELECT gen_random_uuid()::text")
        .fetch_one(&mut c)
        .await?;
    let l = Lease {
        source: "owned_c1_typed".into(),
        task,
        generation: 1,
        epoch: 1,
    };
    sqlx::query("INSERT INTO jobs.tasks(id,project_id,kind,payload_version,payload,config_version,artifact_digest,dedupe_key,state,run_at,attempt,max_attempts,lease_owner,lease_until,generation) VALUES($1::uuid,gen_random_uuid(),'sync',1,'{}','owned',$2,'owned','running',now(),1,3,'owned',now()+interval '1 hour',1)")
        .bind(&l.task).bind("a".repeat(64)).execute(&mut c).await?;
    let measured = format!("mysql.{}", cap.l0.split(':').next().unwrap());
    sqlx::query("INSERT INTO _oo.source_instance(source,source_epoch,engine,measured_id,config_digest,schema_digest,shard_id,owner_token,lease_id,lease_generation) VALUES($1,1,'mysql',$2,$3,$3,'owned_c1',gen_random_uuid(),$4::uuid,1)")
        .bind(&l.source).bind(&measured).bind("a".repeat(64)).bind(&l.task).execute(&mut c).await?;
    sqlx::query("SELECT _oo.native_c1_baseline_v2_open($1,$2::uuid,1,1,$3,$4,$4,$5,$6::numeric)")
        .bind(&l.source)
        .bind(&l.task)
        .bind(&measured)
        .bind("a".repeat(64))
        .bind(&cap.l0)
        .bind(cap.initial_position().to_string())
        .execute(&mut c)
        .await?;
    sql(&mut c, "SET ROLE oo_pipeline_runtime").await?;
    for (name, spec) in cap.specs() {
        c1_typed::open(&mut c, &l, &name, &spec).await?;
    }
    Ok((c, l))
}

// Independent source/target SQL oracle: no shared key codec or product digest.
async fn audit(source: &mut Conn, target: &mut PgConnection, l: &Lease) -> Result<Value> {
    let mut all = serde_json::Map::new();
    let evidence = PathBuf::from(std::env::var("OO_C1_TYPED_EVIDENCE")?).join(format!(
        "independent-audit-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir(&evidence)?;
    for table in ["main_rows", "string_rows", "integer_rows", "binary_rows"] {
        let source_sql = match table {
            "main_rows" => {
                "SELECT CAST(n AS CHAR),code,payload,CAST(amount AS CHAR),LOWER(HEX(raw)) FROM c1_typed.main_rows"
            }
            "string_rows" => "SELECT code,payload FROM c1_typed.string_rows",
            "integer_rows" => "SELECT CAST(id AS CHAR),payload FROM c1_typed.integer_rows",
            _ => "SELECT LOWER(HEX(code)),payload FROM c1_typed.binary_rows",
        };
        let source_rows: Vec<mysql_async::Row> = source.query(source_sql).await?;
        let mut expected = Vec::<Vec<Option<String>>>::new();
        for row in source_rows {
            expected.push(
                (0..row.len())
                    .map(|i| row.get::<Option<String>, _>(i).unwrap())
                    .collect(),
            );
        }
        let physical: String = sqlx::query_scalar(
            "SELECT physical FROM _oo.c1_typed_table WHERE source=$1 AND task=$2::uuid AND name=$3",
        )
        .bind(&l.source)
        .bind(&l.task)
        .bind(format!("c1_typed.{table}"))
        .fetch_one(&mut *target)
        .await?;
        assert!(physical.starts_with("c3p_") && physical.len() == 36);
        let cols = match table {
            "main_rows" => {
                "jsonb_build_array(c0::text,c1::text,c2::text,c3::text,encode(c4,'hex'))"
            }
            "binary_rows" => "jsonb_build_array(encode(c0,'hex'),c1::text)",
            _ => "jsonb_build_array(c0::text,c1::text)",
        };
        let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT {cols}::text FROM _oo.{physical}"
        )))
        .fetch_all(&mut *target)
        .await?;
        let mut actual = rows
            .into_iter()
            .map(|s| serde_json::from_str::<Vec<Option<String>>>(&s))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        expected.sort();
        actual.sort();
        fs::write(
            evidence.join(format!("{table}-source.json")),
            serde_json::to_vec(&expected)?,
        )?;
        fs::write(
            evidence.join(format!("{table}-target.json")),
            serde_json::to_vec(&actual)?,
        )?;
        assert!(
            actual == expected,
            "independent typed rows differ: {table}; raw rows retained at {}",
            evidence.display()
        );
        all.insert(table.into(),json!({"rows":actual.len(),"source_sha256":ti::sha256_hex(&serde_json::to_vec(&expected)?),
            "target_sha256":ti::sha256_hex(&serde_json::to_vec(&actual)?),"differences":0}));
    }
    fs::write(
        evidence.join("manifest.json"),
        serde_json::to_vec_pretty(&all)?,
    )?;
    Ok(Value::Object(all))
}

#[tokio::test]
#[ignore = "requires newly owned MySQL 23327 and PG 25487; no customer resources"]
async fn real_binlog_typed_10k_iud_and_ack_recovery() -> Result<()> {
    let ev = PathBuf::from(std::env::var("OO_C1_TYPED_EVIDENCE")?);
    fs::create_dir_all(&ev)?;
    let run = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let out = home().join(format!("capture-{run}"));
    let db = format!("c1typed_{run}");
    let mut source = mysql().await?;
    let version: String = source
        .query_first("SELECT VERSION()")
        .await?
        .ok_or("version")?;
    assert!(version.starts_with("5.7.44"));
    // Fixture-only reset. This named database belongs to this test, not a shared/customer DB.
    for table in [
        "main_rows",
        "string_rows",
        "integer_rows",
        "binary_rows",
        "no_key",
    ] {
        source
            .query_drop(format!("DROP TABLE IF EXISTS c1_typed.{table}"))
            .await?;
    }
    source.query_drop("CREATE TABLE c1_typed.main_rows(n BIGINT NOT NULL,code VARCHAR(32) NOT NULL,payload VARCHAR(64),amount DECIMAL(12,2) NOT NULL,raw VARBINARY(16) NOT NULL,PRIMARY KEY(code,n)) ENGINE=InnoDB").await?;
    source.query_drop("CREATE TABLE c1_typed.string_rows(code VARCHAR(32) PRIMARY KEY,payload VARCHAR(64)) ENGINE=InnoDB").await?;
    source.query_drop("CREATE TABLE c1_typed.integer_rows(id BIGINT UNSIGNED PRIMARY KEY,payload VARCHAR(64)) ENGINE=InnoDB").await?;
    source.query_drop("CREATE TABLE c1_typed.binary_rows(code VARBINARY(16) PRIMARY KEY,payload VARCHAR(64)) ENGINE=InnoDB").await?;
    source
        .query_drop("CREATE TABLE c1_typed.no_key(n INT,payload VARCHAR(64)) ENGINE=InnoDB")
        .await?;
    for start in (1..=10_000).step_by(1000) {
        let values = (start..start + 1000)
            .map(|n| format!("({n},'key{n:05}','before-{n}',{n}.25,UNHEX('00ff'))"))
            .collect::<Vec<_>>()
            .join(",");
        source
            .query_drop(format!("INSERT INTO c1_typed.main_rows VALUES {values}"))
            .await?;
    }
    source.query_drop("INSERT INTO c1_typed.string_rows VALUES('Z','z'),('a','a'),('b',NULL),('','empty'),(CONVERT(UNHEX('c3a9') USING utf8mb4),'unicode')").await?;
    source
        .query_drop(
            "INSERT INTO c1_typed.integer_rows VALUES(1,'one'),(18446744073709551615,'max')",
        )
        .await?;
    source
        .query_drop(
            "INSERT INTO c1_typed.binary_rows VALUES(UNHEX('00ff'),'binary'),(UNHEX('61'),'ascii')",
        )
        .await?;
    let config = home().join(format!("config-{run}.json"));
    let columns = json!({"c1_typed.main_rows":["n","code","payload","amount","raw"],
        "c1_typed.string_rows":["code","payload"],"c1_typed.integer_rows":["id","payload"],"c1_typed.binary_rows":["code","payload"]});
    fs::write(
        &config,
        serde_json::to_vec(
            &json!({"database":"c1_typed","host":"127.0.0.1","port":23327,
        "user":"oo_cdc","password_file":home().join("secrets/cdc.pw"),"ca_file":home().join("certs/ca.pem"),
        "tls_server_name":"localhost","server_id":5728,"health_every_ms":200,"reconcile_every_ms":1000,
        "preserve_dir":out,"fixture":"owned-c1-typed","tables":columns.as_object().unwrap().keys().collect::<Vec<_>>(),"columns":columns}),
        )?,
    )?;
    let mut capture = Capture::open(&config, &out, "owned_c1_typed", 1).await?;
    let (mut runtime, lease) = setup_target(&db, &capture).await?;
    let mut admin = pg(&db).await?;
    let mut sequence = 1;
    let mut fills = Vec::new();
    for name in capture.specs().keys() {
        let mut lo = None;
        while let Some((batch, hi)) = capture
            .snapshot_chunk(name, lo.clone(), 2500, sequence)
            .await?
        {
            apply_ack(&mut runtime, &lease, &batch).await?;
            fills.push(batch);
            lo = Some(hi);
            sequence += 1;
        }
    }
    let baseline = audit(&mut source, &mut admin, &lease).await?;
    assert_eq!(baseline["main_rows"]["rows"], 10_000);
    // All 4000 INSERTs are one committed multi-table transaction.
    source.query_drop("START TRANSACTION").await?;
    for start in (10_001..=14_000).step_by(1000) {
        let values = (start..start + 1000)
            .map(|n| format!("({n},'key{n:05}','insert-{n}',{n}.50,UNHEX('ff00'))"))
            .collect::<Vec<_>>()
            .join(",");
        source
            .query_drop(format!("INSERT INTO c1_typed.main_rows VALUES {values}"))
            .await?;
    }
    source
        .query_drop("UPDATE c1_typed.string_rows SET payload='txn' WHERE code='a'")
        .await?;
    source.query_drop("COMMIT").await?;
    let first = capture.next(sequence, Duration::from_secs(30)).await?;
    assert_eq!(first.parts.iter().map(|p| p.rows).sum::<usize>(), 4001);
    // Fail on the second table AFTER the first table's COPY/apply. Whole TX rolls back.
    let mut bad = first.clone();
    bad.parts.last_mut().unwrap().spec.columns[1].typmod -= 1;
    assert!(c1_typed::apply(&mut runtime, &lease, &bad).await.is_err());
    let receipt_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo.c1_typed_receipt WHERE seq=$1")
            .bind(sequence)
            .fetch_one(&mut admin)
            .await?;
    assert_eq!(receipt_count, 0);
    let main_physical: String = sqlx::query_scalar(
        "SELECT physical FROM _oo.c1_typed_table WHERE name='c1_typed.main_rows'",
    )
    .fetch_one(&mut admin)
    .await?;
    let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM _oo.{main_physical}"
    )))
    .fetch_one(&mut admin)
    .await?;
    assert_eq!(
        count, 10_000,
        "first table rolled back with second table refusal"
    );
    let r = c1_typed::apply(&mut runtime, &lease, &first).await?;
    let mut next = first.clone();
    next.sequence += 1;
    next.previous_position = first.position.clone();
    next.position = (first.position.parse::<u64>()? + 1).to_string();
    next.parts.clear();
    assert!(
        c1_typed::apply(&mut runtime, &lease, &next).await.is_err(),
        "no advance before checkpoint CAS"
    );
    drop(runtime); // target commit, process/ACK loss, new connection
    runtime = pg(&db).await?;
    sql(&mut runtime, "SET ROLE oo_pipeline_runtime").await?;
    let replay = c1_typed::apply(&mut runtime, &lease, &first).await?;
    assert!(replay.replay);
    assert_eq!(r.digest, replay.digest);
    let mut wrong = replay.clone();
    wrong.digest = "0".repeat(64);
    assert!(!ack(&mut runtime, &lease, &wrong).await?);
    assert!(ack(&mut runtime, &lease, &replay).await?);
    assert!(ack(&mut runtime, &lease, &replay).await?); // lost ACK after CAS
    sequence += 1;
    source.query_drop("UPDATE c1_typed.main_rows SET payload=CONCAT('updated-',n),amount=amount+1 WHERE n BETWEEN 1 AND 4000").await?;
    let updates = capture.next(sequence, Duration::from_secs(30)).await?;
    assert_eq!(updates.parts[0].rows, 4000);
    apply_ack(&mut runtime, &lease, &updates).await?;
    sequence += 1;
    source
        .query_drop("DELETE FROM c1_typed.main_rows WHERE n BETWEEN 4001 AND 6000")
        .await?;
    let deletes = capture.next(sequence, Duration::from_secs(30)).await?;
    assert_eq!(deletes.parts[0].rows, 2000);
    apply_ack(&mut runtime, &lease, &deletes).await?;
    sequence += 1;
    let after_iud = audit(&mut source, &mut admin, &lease).await?;
    assert_eq!(after_iud["main_rows"]["rows"], 12000);
    // A late snapshot part cannot resurrect deleted keys or overwrite CDC rows.
    for fill in fills
        .iter()
        .filter(|b| b.parts[0].table == "c1_typed.main_rows")
    {
        let mut late = fill.clone();
        late.sequence = sequence;
        late.previous_position = deletes.position.clone();
        late.position = deletes.position.clone();
        apply_ack(&mut runtime, &lease, &late).await?;
        sequence += 1;
    }
    audit(&mut source, &mut admin, &lease).await?;
    source.query_drop("START TRANSACTION").await?;
    source
        .query_drop("UPDATE c1_typed.main_rows SET payload='must-not-capture' WHERE n=2")
        .await?;
    source.query_drop("ROLLBACK").await?;
    source.query_drop("START TRANSACTION").await?;
    source
        .query_drop("UPDATE c1_typed.main_rows SET code='moved' WHERE n=1")
        .await?;
    source
        .query_drop("DELETE FROM c1_typed.string_rows WHERE code='b'")
        .await?;
    source
        .query_drop("INSERT INTO c1_typed.string_rows VALUES('b','reborn')")
        .await?;
    source
        .query_drop(
            "UPDATE c1_typed.integer_rows SET payload='unsigned' WHERE id=18446744073709551615",
        )
        .await?;
    source.query_drop("UPDATE c1_typed.binary_rows SET code=UNHEX('ff00'),payload='moved-bytes' WHERE code=UNHEX('00ff')").await?;
    source.query_drop("COMMIT").await?;
    let edges = capture.next(sequence, Duration::from_secs(30)).await?;
    let final_receipt = apply_ack(&mut runtime, &lease, &edges).await?;
    let final_audit = audit(&mut source, &mut admin, &lease).await?;
    // Runtime cannot read private stage/target or bypass the live lease.
    assert!(
        sqlx::query("SELECT * FROM _oo.c1_typed_table")
            .fetch_all(&mut runtime)
            .await
            .is_err()
    );
    let mut wrong_lease = lease.clone();
    wrong_lease.generation += 1;
    assert!(
        c1_typed::apply(&mut runtime, &wrong_lease, &edges)
            .await
            .is_err()
    );
    sqlx::query("UPDATE jobs.tasks SET lease_until=now()-interval '1 second' WHERE id=$1::uuid")
        .bind(&lease.task)
        .execute(&mut admin)
        .await?;
    assert!(ack(&mut runtime, &lease, &final_receipt).await.is_err());
    sqlx::query("UPDATE jobs.tasks SET lease_until=now()+interval '1 hour' WHERE id=$1::uuid")
        .bind(&lease.task)
        .execute(&mut admin)
        .await?;
    let mut unkeyed: Value = serde_json::from_slice(&fs::read(&config)?)?;
    unkeyed["tables"] = json!(["c1_typed.no_key"]);
    unkeyed["columns"] = json!({"c1_typed.no_key":["n","payload"]});
    let unkeyed_config = home().join(format!("no-key-{run}.json"));
    fs::write(&unkeyed_config, serde_json::to_vec(&unkeyed)?)?;
    let error = match Capture::open(
        &unkeyed_config,
        &home().join(format!("no-key-capture-{run}")),
        "owned_c1_typed",
        1,
    )
    .await
    {
        Err(e) => e,
        Ok(_) => panic!("unkeyed CDC must refuse"),
    };
    assert_eq!(error.fact, "no_primary_key_cdc_use_c5plus_or_full");
    let writes:u64=source.query_first("SELECT COUNT(*) FROM mysql.general_log WHERE user_host LIKE 'oo_cdc[%' AND command_type='Query' AND argument REGEXP '^[[:space:]]*(INSERT|UPDATE|DELETE|CREATE|DROP|ALTER|TRUNCATE|REPLACE|GRANT|REVOKE)[[:space:]]'").await?.ok_or("reader audit")?;
    assert_eq!(writes, 0);
    fs::write(
        ev.join("last-receipt.json"),
        serde_json::to_vec_pretty(&final_receipt)?,
    )?;
    fs::write(
        ev.join("live-result.json"),
        serde_json::to_vec_pretty(&json!({
            "source":"real MySQL ROW/FULL/GTID","version":version,"target_database":db,"fixture_home":home(),
            "baseline":baseline,"after_4000i_4000u_2000d":after_iud,"after_edges":final_audit,
            "reader_write_statements":writes,"initial_position":capture.l0,"final_position":edges.position,
            "fill_parts":fills.len(),"target_commit_ack_replay":true,"multi_table_rollback":true,
            "runtime_read_denied":true,"stale_and_expired_lease_denied":true,
            "publication":false,"source_onboarding":false,"scheduler_control_cas":false
        }))?,
    )?;
    Ok(())
}
