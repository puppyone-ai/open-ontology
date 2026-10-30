use job_worker::native_schedule::c5plus_policy::{Config, Progress};

#[tokio::test]
async fn owned_control_schema_upgrade_replay_and_durable_layer_deadlines() {
    let port = std::env::var("OO_C5_CONTROL_PORT")
        .unwrap_or_else(|_| "25461".into())
        .parse::<u16>()
        .unwrap();
    let password = std::env::var("OO_C5_CONTROL_PASSWORD_FILE")
        .map(|p| std::fs::read_to_string(p).unwrap())
        .unwrap_or_else(|_| "fixture-only".into());
    let (client, connection) = tokio_postgres::connect(
        &format!(
            "host=127.0.0.1 port={port} user=postgres password={} dbname=postgres",
            password.trim()
        ),
        tokio_postgres::NoTls,
    )
    .await
    .unwrap();
    let task = tokio::spawn(async move { connection.await.unwrap() });
    client.batch_execute("CREATE SCHEMA IF NOT EXISTS jobs; CREATE TABLE IF NOT EXISTS jobs.tasks(id uuid PRIMARY KEY); DO $$ BEGIN IF NOT EXISTS(SELECT FROM pg_roles WHERE rolname='oo_jobs_runtime') THEN CREATE ROLE oo_jobs_runtime NOLOGIN; END IF; END $$;").await.unwrap();
    let script = include_str!("../native_schedule.sql");
    let old = script
        .split("CREATE TABLE IF NOT EXISTS jw_native.c5_schedule")
        .next()
        .unwrap();
    client.batch_execute(old).await.unwrap();
    client.batch_execute(script).await.unwrap();
    client.batch_execute(script).await.unwrap();
    let binding = "00000000-0000-4000-8000-000000000001";
    client.execute("INSERT INTO jw_native.bindings(id,project_id,source_id,method,cycle,period_ms,manifest_sha,manifest) VALUES($1::text::uuid,$1::text::uuid,'fixture','C5','reconcile',1000,repeat('a',64),'{}') ON CONFLICT DO NOTHING",&[&binding]).await.unwrap();
    client.execute("INSERT INTO jw_native.c5_schedule(binding_id) VALUES($1::text::uuid) ON CONFLICT DO NOTHING",&[&binding]).await.unwrap();
    client
        .execute(
            "INSERT INTO jobs.tasks(id) VALUES($1::text::uuid) ON CONFLICT DO NOTHING",
            &[&binding],
        )
        .await
        .unwrap();
    for stage in [
        "c5_initialize_full_if_absent",
        "c5_export",
        "c5_export_before",
        "c5_export_after",
        "c5_observe",
    ] {
        client.execute("INSERT INTO jw_native.process_attempts(task_id,generation,stage,executable_sha256,exit_code,stdout,stderr) VALUES($1::text::uuid,1,$2,repeat('a',64),0,'{}'::bytea,''::bytea) ON CONFLICT DO NOTHING", &[&binding,&stage]).await.unwrap();
    }
    assert!(client.execute("INSERT INTO jw_native.process_attempts(task_id,generation,stage,executable_sha256,exit_code,stdout,stderr) VALUES($1::text::uuid,1,'c5_arbitrary',repeat('a',64),0,'{}'::bytea,''::bytea)", &[&binding]).await.is_err());
    let config = Config {
        period_ms: [1000, 2000, 3000, 4000],
        duty_per_mille: 250,
        threads_running_max: 8,
        execution_window_utc: [0, 1440],
    };
    let progress = Progress {
        completed_at_ms: [Some(9000), Some(8000), Some(7000), Some(0)],
    };
    let due = config.due(10000, 0, 8, &progress).unwrap();
    client.execute("UPDATE jw_native.c5_schedule SET progress=$2::text::jsonb,due=$3::text::jsonb,observed_at_ms=10000 WHERE binding_id=$1::text::uuid",&[&binding,&serde_json::to_string(&progress).unwrap(),&serde_json::to_string(&due).unwrap()]).await.unwrap();
    let row=client.query_one("SELECT progress::text,due::text FROM jw_native.c5_schedule WHERE binding_id=$1::text::uuid",&[&binding]).await.unwrap();
    let restored: Progress = serde_json::from_str(&row.get::<_, String>(0)).unwrap();
    assert_eq!(restored, progress);
    assert_eq!(
        serde_json::from_str::<[bool; 4]>(&row.get::<_, String>(1)).unwrap(),
        [true; 4]
    );
    assert_eq!(config.due(10000, 0, 9, &restored).unwrap(), [false; 4]);
    drop(client);
    task.await.unwrap();
}
