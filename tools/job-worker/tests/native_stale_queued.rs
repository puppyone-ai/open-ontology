use job_worker::native_schedule::reject_stale_queued;
use tokio_postgres::NoTls;

#[tokio::test]
async fn due_orphan_is_explicit_but_future_and_owned_tasks_are_not_stale() {
    let port = std::env::var("OO_RB_PG_PORT").expect("owned fixture port");
    let (client, connection) = tokio_postgres::connect(
        &format!("host=127.0.0.1 port={port} user=postgres dbname=postgres"),
        NoTls,
    )
    .await
    .unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    client.batch_execute("ALTER TABLE jobs.tasks ADD COLUMN IF NOT EXISTS project_id uuid, ADD COLUMN IF NOT EXISTS kind text, ADD COLUMN IF NOT EXISTS state text, ADD COLUMN IF NOT EXISTS run_at timestamptz;").await.unwrap();
    let project = "00000000-0000-4000-8000-000000000010";
    client.batch_execute("DELETE FROM jw_native.occurrences WHERE binding_id='00000000-0000-4000-8000-000000000030'").await.unwrap();
    client.batch_execute("INSERT INTO jobs.tasks(id,project_id,kind,state,run_at) VALUES('00000000-0000-4000-8000-000000000011','00000000-0000-4000-8000-000000000010','native_capture','queued',clock_timestamp()+interval '1 hour') ON CONFLICT(id) DO UPDATE SET state='queued',run_at=EXCLUDED.run_at").await.unwrap();
    reject_stale_queued(&client, project).await.unwrap();
    client.batch_execute("UPDATE jobs.tasks SET run_at=clock_timestamp()-interval '1 hour' WHERE id='00000000-0000-4000-8000-000000000011'").await.unwrap();
    assert_eq!(
        reject_stale_queued(&client, project)
            .await
            .unwrap_err()
            .kind,
        "stale_queued_task"
    );
    client.batch_execute("UPDATE jobs.tasks SET state='cancelled' WHERE id='00000000-0000-4000-8000-000000000011'").await.unwrap();
    reject_stale_queued(&client, project).await.unwrap();
    client.batch_execute("INSERT INTO jw_native.bindings(id,project_id,source_id,method,cycle,period_ms,manifest_sha,manifest) VALUES('00000000-0000-4000-8000-000000000030','00000000-0000-4000-8000-000000000010','sample','C5','reconcile',1000,repeat('a',64),'{}') ON CONFLICT DO NOTHING; INSERT INTO jw_native.occurrences(binding_id,pin,task_id,manifest_sha,scheduled_at,manifest) VALUES('00000000-0000-4000-8000-000000000030',1,'00000000-0000-4000-8000-000000000011',repeat('a',64),clock_timestamp(),'{}') ON CONFLICT DO NOTHING; UPDATE jobs.tasks SET state='queued' WHERE id='00000000-0000-4000-8000-000000000011';").await.unwrap();
    reject_stale_queued(&client, project).await.unwrap();
    client.batch_execute("UPDATE jobs.tasks SET state='cancelled' WHERE id='00000000-0000-4000-8000-000000000011'").await.unwrap();
    reject_stale_queued(&client, "00000000-0000-4000-8000-000000000020")
        .await
        .unwrap();
}
