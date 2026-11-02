use job_worker::{config::Config, native_schedule, session};
use std::path::PathBuf;

#[tokio::test]
#[ignore = "requires OO_INITIAL_CONTROL_CONFIG pointing to a new owned control database"]
async fn extended_process_stages_survive_normal_reinstall() {
    let path = PathBuf::from(std::env::var("OO_INITIAL_CONTROL_CONFIG").unwrap());
    let config = Config::load(&path).unwrap();
    assert!(config.database.starts_with("worker_initial_hooks_"));
    let client = session::admin_db(&config, &config.database).await.unwrap();
    client.batch_execute("CREATE SCHEMA jobs; CREATE TABLE jobs.tasks(id uuid PRIMARY KEY);").await.unwrap();
    native_schedule::install_schema(&client).await.unwrap();
    let task = "00000000-0000-4000-8000-000000000098";
    client.execute("INSERT INTO jobs.tasks VALUES($1::text::uuid)", &[&task]).await.unwrap();
    for stage in ["loader_c1_export_meta", "loader_c1_export_page_000000",
        "loader_c1_initial_resume", "loader_c1_initial_end", "normalization_rejects_999",
        "loader_c1_stage_snapshot", "ack"] {
        client.execute("INSERT INTO jw_native.process_attempts(task_id,generation,stage,executable_sha256,exit_code,stdout,stderr) VALUES($1::text::uuid,1,$2,repeat('a',64),0,'{}'::bytea,''::bytea)", &[&task,&stage]).await.unwrap();
    }
    let snapshot = "SELECT jsonb_agg(to_jsonb(a) ORDER BY stage)::text FROM jw_native.process_attempts a";
    let before: String = client.query_one(snapshot, &[]).await.unwrap().get(0);
    let guard = "SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conrelid='jw_native.process_attempts'::regclass AND conname='process_attempts_stage_check'";
    let original: String = client.query_one(guard, &[]).await.unwrap().get(0);
    for _ in 0..2 {
        native_schedule::install_schema(&client).await.unwrap();
        let after: String = client.query_one(snapshot, &[]).await.unwrap().get(0);
        let installed: String = client.query_one(guard, &[]).await.unwrap().get(0);
        assert_eq!(before, after);
        assert_eq!(original, installed);
    }
    for stage in ["loader_c1_initial_arbitrary", "loader_c1_initial_resume_1",
        "loader_c1_export_page_1000000", "normalization_rejects_1000"] {
        let error = client.execute("INSERT INTO jw_native.process_attempts(task_id,generation,stage,executable_sha256,exit_code,stdout,stderr) VALUES($1::text::uuid,1,$2,repeat('a',64),0,'{}'::bytea,''::bytea)", &[&task,&stage]).await.unwrap_err();
        assert_eq!(error.code().unwrap().code(), "23514");
    }
}
