use job_worker::native_schedule::install_schema;
use tokio_postgres::{Client, NoTls};

async fn connection() -> Client {
    let port = std::env::var("OO_RB_PG_PORT").expect("OO_RB_PG_PORT owned fixture");
    let (client, connection) = tokio_postgres::connect(
        &format!("host=127.0.0.1 port={port} user=postgres dbname=postgres"),
        NoTls,
    )
    .await
    .unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    client
}

#[tokio::test]
async fn schema_install_waits_for_target_installer_and_replays_concurrently() {
    let mut owner = connection().await;
    owner.batch_execute("CREATE SCHEMA IF NOT EXISTS jobs; CREATE TABLE IF NOT EXISTS jobs.tasks(id uuid PRIMARY KEY); DO $$ BEGIN IF NOT EXISTS(SELECT FROM pg_roles WHERE rolname='oo_jobs_runtime') THEN CREATE ROLE oo_jobs_runtime NOLOGIN; END IF; END $$;").await.unwrap();
    let tx = owner.transaction().await.unwrap();
    tx.batch_execute("SELECT pg_advisory_xact_lock(6404202605)")
        .await
        .unwrap();
    let client = connection().await;
    let mut waiting = tokio::spawn(async move { install_schema(&client).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(300), &mut waiting)
            .await
            .is_err()
    );
    tx.commit().await.unwrap();
    waiting.await.unwrap().unwrap();
    let mut handles = Vec::new();
    for _ in 0..6 {
        let client = connection().await;
        handles.push(tokio::spawn(async move { install_schema(&client).await }));
    }
    for handle in handles {
        handle.await.unwrap().unwrap();
    }
}
