use tokio_postgres::Client;

use crate::api::{LeaseStatus, TaskRequest, CONFIG_VERSION, KIND, WAVE2_DELAY_SECONDS};
use crate::error::{Error, Result};
use crate::lease;
use crate::session;
use crate::sql;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireStatus {
    Fired,
    Duplicate,
}

pub async fn register(client: &mut Client, request: &TaskRequest, dedupe_prefix: &str) -> Result<String> {
    request.check()?;
    if !(1..=200).contains(&dedupe_prefix.len()) || !dedupe_prefix.is_ascii() {
        return Err(Error::new("input"));
    }
    let tx = client.transaction().await.map_err(Error::sql)?;
    let id: String = tx
        .query_one(
            "INSERT INTO jobs.schedules \
             (project_id, kind, config_version, artifact_digest, dedupe_prefix, next_run_at) \
             VALUES ($1::text::uuid, $2, $3, $4, $5, clock_timestamp()) \
             RETURNING id::text",
            &[
                &request.project_id,
                &KIND,
                &CONFIG_VERSION,
                &request.digest(),
                &dedupe_prefix,
            ],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    tx.execute(
        "INSERT INTO jw.schedule_payloads \
         (schedule_id, project_id, payload, payload_version, config_version, artifact_digest, max_attempts) \
         VALUES ($1::text::uuid, $2::text::uuid, $3, 1, $4, $5, $6)",
        &[
            &id,
            &request.project_id,
            &request.payload_text(),
            &CONFIG_VERSION,
            &request.digest(),
            &request.max_attempts,
        ],
    )
    .await
    .map_err(Error::sql)?;
    tx.commit().await.map_err(Error::sql)?;
    Ok(id)
}

pub async fn fire_race(config: &crate::config::Config, schedule_id: &str) -> Result<()> {
    for _ in 0..3 {
        let mut left = session::runtime(config).await?;
        let mut right = session::runtime(config).await?;
        let (first, second) = tokio::join!(
            fire_once(&mut left, schedule_id),
            fire_once(&mut right, schedule_id)
        );
        let occurrences: i64 = left
            .query_one(
                "SELECT count(*)::bigint FROM jobs.schedule_occurrences WHERE schedule_id = $1::text::uuid",
                &[&schedule_id],
            )
            .await
            .map_err(Error::sql)?
            .get(0);
        if occurrences > 1 || matches!((&first, &second), (Ok(FireStatus::Fired), Ok(FireStatus::Fired)))
        {
            return Err(Error::new("schedule"));
        }
        if occurrences == 1 {
            return Ok(());
        }
        if pair_retryable(&first, &second) {
            eprintln!("event=schedule_retry");
            continue;
        }
        return Err(Error::new("schedule"));
    }
    Err(Error::new("schedule"))
}

fn pair_retryable(first: &Result<FireStatus>, second: &Result<FireStatus>) -> bool {
    side_retryable(first) && side_retryable(second)
}

fn side_retryable(side: &Result<FireStatus>) -> bool {
    match side {
        Ok(FireStatus::Duplicate) => true,
        Ok(FireStatus::Fired) => false,
        Err(err) => err.is_deadlock() || err.state.as_deref() == Some("OO016"),
    }
}

async fn fire_once(client: &mut Client, schedule_id: &str) -> Result<FireStatus> {
    let tx = client.transaction().await.map_err(Error::sql)?;
    let schedule = tx
        .query_opt(
            "SELECT next_run_at::text, project_id::text \
             FROM jobs.schedules WHERE id = $1::text::uuid AND enabled",
            &[&schedule_id],
        )
        .await
        .map_err(Error::sql)?;
    let Some(schedule) = schedule else {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("schedule"));
    };
    let at: String = schedule.try_get(0).map_err(Error::sql)?;
    let project_id: String = schedule.try_get(1).map_err(Error::sql)?;
    let stored = tx
        .query_one(
            "SELECT payload, config_version, artifact_digest, max_attempts \
             FROM jw.schedule_payloads WHERE schedule_id = $1::text::uuid AND project_id = $2::text::uuid",
            &[&schedule_id, &project_id],
        )
        .await
        .map_err(Error::sql)?;
    let payload: String = stored.try_get(0).map_err(Error::sql)?;
    let config_version: String = stored.try_get(1).map_err(Error::sql)?;
    let digest: String = stored.try_get(2).map_err(Error::sql)?;
    let max_attempts: i32 = stored.try_get(3).map_err(Error::sql)?;
    let dedupe: String = tx
        .query_one(
            "SELECT 'sched:' || $1::text::uuid::text || ':' || \
             to_char($2::text::timestamptz AT TIME ZONE 'UTC', 'YYYYMMDDHH24MISSUS')",
            &[&schedule_id, &at],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    let enqueued = match lease::enqueue(
        &tx,
        lease::Enqueue {
            project_id: &project_id,
            kind: KIND,
            payload_version: 1,
            payload: &payload,
            config_version: &config_version,
            digest: &digest,
            dedupe_key: &dedupe,
            run_delay_seconds: WAVE2_DELAY_SECONDS as f64,
            max_attempts,
        },
    )
    .await
    {
        Ok(result) => result,
        Err(err) if err.is_deadlock() || err.is_unique() => {
            tx.rollback().await.map_err(Error::sql)?;
            return if err.is_unique() {
                Ok(FireStatus::Duplicate)
            } else {
                Err(err)
            };
        }
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };
    if enqueued.status != LeaseStatus::Enqueued && enqueued.status != LeaseStatus::Replayed {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("schedule"));
    }
    let Some(task_id) = enqueued.task_id else {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("schedule"));
    };
    match record_occurrence(tx, schedule_id, &at, &project_id, &task_id).await? {
        Occurrence::Fired => Ok(FireStatus::Fired),
        Occurrence::Repair => {
            repair_next(client, schedule_id).await?;
            Ok(FireStatus::Duplicate)
        }
    }
}

enum Occurrence {
    Fired,
    Repair,
}

async fn record_occurrence(
    tx: tokio_postgres::Transaction<'_>,
    schedule_id: &str,
    at: &str,
    project_id: &str,
    task_id: &str,
) -> Result<Occurrence> {
    tx.execute(sql::NOTIFY, &[&project_id])
        .await
        .map_err(Error::sql)?;
    if let Err(err) = tx
        .execute(
            "INSERT INTO jobs.schedule_occurrences \
             (schedule_id, scheduled_at, project_id, task_id) \
             VALUES ($1::text::uuid, $2::text::timestamptz, $3::text::uuid, $4::text::uuid)",
            &[&schedule_id, &at, &project_id, &task_id],
        )
        .await
    {
        let mapped = Error::sql(err);
        tx.rollback().await.map_err(Error::sql)?;
        if mapped.is_unique() {
            return Ok(Occurrence::Repair);
        }
        if mapped.is_deadlock() {
            return Err(mapped);
        }
        return Err(mapped);
    }
    tx.execute(
        "UPDATE jobs.schedules \
         SET next_run_at = next_run_at + interval '1 hour' \
         WHERE id = $1::text::uuid AND next_run_at = $2::text::timestamptz",
        &[&schedule_id, &at],
    )
    .await
    .map_err(Error::sql)?;
    tx.commit().await.map_err(Error::sql)?;
    Ok(Occurrence::Fired)
}

async fn repair_next(client: &Client, schedule_id: &str) -> Result<()> {
    client
        .execute(
            "UPDATE jobs.schedules AS schedule \
             SET next_run_at = occurrence.scheduled_at + interval '1 hour' \
             FROM jobs.schedule_occurrences AS occurrence \
             WHERE schedule.id = occurrence.schedule_id \
               AND schedule.id = $1::text::uuid \
               AND schedule.next_run_at = occurrence.scheduled_at",
            &[&schedule_id],
        )
        .await
        .map_err(Error::sql)?;
    Ok(())
}
