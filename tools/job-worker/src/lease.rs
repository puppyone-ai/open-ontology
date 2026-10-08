use tokio_postgres::GenericClient;

use crate::api::{LeaseResult, LeaseStatus};
use crate::error::{Error, Result};
use crate::sql;

pub async fn one(
    client: &impl GenericClient,
    statement: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<LeaseResult> {
    let row = client.query_one(statement, params).await.map_err(Error::sql)?;
    let status_text: String = row.try_get(0).map_err(Error::sql)?;
    Ok(LeaseResult {
        status: LeaseStatus::parse(&status_text)?,
        task_id: row.try_get(1).map_err(Error::sql)?,
        project_id: row.try_get(2).map_err(Error::sql)?,
        kind: row.try_get(3).map_err(Error::sql)?,
        state: row.try_get(4).map_err(Error::sql)?,
        attempt: row.try_get(5).map_err(Error::sql)?,
        generation: row.try_get(6).map_err(Error::sql)?,
        lease_owner: row.try_get(7).map_err(Error::sql)?,
        run_at: row.try_get(8).map_err(Error::sql)?,
    })
}

pub struct Enqueue<'a> {
    pub project_id: &'a str,
    pub kind: &'a str,
    pub payload_version: i32,
    pub payload: &'a str,
    pub config_version: &'a str,
    pub digest: &'a str,
    pub dedupe_key: &'a str,
    pub run_delay_seconds: f64,
    pub max_attempts: i32,
}

pub async fn enqueue(client: &impl GenericClient, call: Enqueue<'_>) -> Result<LeaseResult> {
    one(
        client,
        sql::ENQUEUE,
        &[
            &call.project_id,
            &call.kind,
            &call.payload_version,
            &call.payload,
            &call.config_version,
            &call.digest,
            &call.dedupe_key,
            &call.run_delay_seconds,
            &call.max_attempts,
        ],
    )
    .await
}

pub async fn claim(
    client: &impl GenericClient,
    project_id: &str,
    kinds: &[String],
    worker: &str,
) -> Result<LeaseResult> {
    one(client, sql::CLAIM, &[&project_id, &kinds, &worker]).await
}

pub async fn heartbeat(
    client: &impl GenericClient,
    project_id: &str,
    task_id: &str,
    worker: &str,
    generation: i64,
) -> Result<LeaseResult> {
    one(
        client,
        sql::HEARTBEAT,
        &[&project_id, &task_id, &worker, &generation],
    )
    .await
}

pub async fn complete_success(
    client: &impl GenericClient,
    project_id: &str,
    task_id: &str,
    worker: &str,
    generation: i64,
) -> Result<LeaseResult> {
    one(
        client,
        sql::COMPLETE_SUCCESS,
        &[&project_id, &task_id, &worker, &generation],
    )
    .await
}

pub async fn complete_failure(
    client: &impl GenericClient,
    project_id: &str,
    task_id: &str,
    worker: &str,
    generation: i64,
    error_kind: &str,
) -> Result<LeaseResult> {
    one(
        client,
        sql::COMPLETE_FAILURE,
        &[&project_id, &task_id, &worker, &generation, &error_kind],
    )
    .await
}

pub async fn retry_failure(
    client: &impl GenericClient,
    project_id: &str,
    task_id: &str,
    worker: &str,
    generation: i64,
    error_kind: &str,
) -> Result<LeaseResult> {
    one(
        client,
        sql::RETRY_FAILURE,
        &[&project_id, &task_id, &worker, &generation, &error_kind],
    )
    .await
}

pub async fn cancel(
    client: &impl GenericClient,
    project_id: &str,
    task_id: &str,
    worker: &str,
    generation: i64,
) -> Result<LeaseResult> {
    one(
        client,
        sql::CANCEL,
        &[&project_id, &task_id, &worker, &generation],
    )
    .await
}
