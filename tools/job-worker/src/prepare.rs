use tokio_postgres::Client;

use crate::api::{LeaseStatus, TaskRequest, KIND, PROOF_PROJECT, WAVE2_DELAY_SECONDS};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::lease;
use crate::schedule;
use crate::session;
use crate::sql;
use crate::worker;

pub async fn prepare(config: &Config) -> Result<()> {
    let mut client = session::runtime(config).await?;
    for index in 0..100 {
        let request = task(
            &format!("bulk-{index:03}"),
            1,
            0,
            None,
            false,
            3,
            0,
        );
        let queued = enqueue(&mut client, &request).await?;
        if queued.status != LeaseStatus::Enqueued {
            return Err(Error::new("prepare"));
        }
    }
    let again = task("bulk-000", 1, 0, None, false, 3, 0);
    let replayed = enqueue(&mut client, &again).await?;
    if replayed.status != LeaseStatus::Replayed {
        return Err(Error::new("prepare"));
    }
    worker::observe(&client, "replayed", 1).await?;

    let delayed = WAVE2_DELAY_SECONDS;
    for request in [
        task("checkpoint", 4, 0, Some(2), false, 4, delayed),
        task("hold-cancel", 1, 0, Some(0), false, 3, delayed),
        task("retry-once", 1, 1, None, false, 3, delayed),
        task("dead-letter", 1, 1, None, false, 1, delayed),
        task("terminal-fail", 1, 0, None, true, 3, delayed),
        task("bad-payload", 0, 0, None, false, 3, delayed),
        task("notify-loss", 1, 0, None, false, 3, delayed),
        task("queued-cancel", 1, 0, None, false, 3, delayed),
    ] {
        let queued = enqueue(&mut client, &request).await?;
        if queued.status != LeaseStatus::Enqueued {
            return Err(Error::new("prepare"));
        }
    }
    cancel_queued(&mut client, "queued-cancel").await?;
    rollback_pair(&mut client).await?;

    let schedule_body = task("once-side-effect", 1, 0, None, false, 3, delayed);
    let schedule_id = schedule::register(&mut client, &schedule_body, "once-side-effect").await?;
    schedule::fire_race(config, &schedule_id).await?;
    let occurrences: i64 = client
        .query_one(
            "SELECT count(*)::bigint FROM jobs.schedule_occurrences WHERE schedule_id = $1::text::uuid",
            &[&schedule_id],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if occurrences != 1 {
        return Err(Error::new("prepare"));
    }

    let notify_state: String = client
        .query_one(
            "SELECT state FROM jobs.tasks WHERE project_id = $1::text::uuid AND dedupe_key = 'notify-loss'",
            &[&PROOF_PROJECT],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if notify_state != "queued" {
        return Err(Error::new("prepare"));
    }
    worker::observe(&client, "notify_left_queued", 1).await?;

    let due: i64 = client
        .query_one(
            "SELECT count(*)::bigint FROM jobs.tasks \
             WHERE project_id = $1::text::uuid AND state = 'queued' AND kind = $2 \
               AND run_at <= clock_timestamp()",
            &[&PROOF_PROJECT, &KIND],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if due != 100 {
        eprintln!("event=prepare_due count={due}");
        return Err(Error::new("prepare"));
    }
    let xact: i64 = client
        .query_one(sql::XACT_COMMIT, &[])
        .await
        .map_err(Error::sql)?
        .get(0);
    worker::observe(&client, "xact_at_prepare", xact).await?;
    eprintln!("event=prepare due={due} xact={xact} schedule={schedule_id}");
    Ok(())
}

fn task(
    dedupe_key: &str,
    units: u32,
    fail_attempts: u32,
    stop_after: Option<u32>,
    terminal_failure: bool,
    max_attempts: i32,
    run_delay_seconds: i64,
) -> TaskRequest {
    TaskRequest {
        project_id: PROOF_PROJECT.to_string(),
        dedupe_key: dedupe_key.to_string(),
        units,
        fail_attempts,
        stop_after,
        terminal_failure,
        max_attempts,
        run_delay_seconds,
    }
}

async fn enqueue(client: &mut Client, request: &TaskRequest) -> Result<crate::api::LeaseResult> {
    request.check()?;
    let payload = request.payload_text();
    let digest = request.digest();
    let tx = client.transaction().await.map_err(Error::sql)?;
    let result = lease::enqueue(
        &tx,
        lease::Enqueue {
            project_id: &request.project_id,
            kind: KIND,
            payload_version: 1,
            payload: &payload,
            config_version: crate::api::CONFIG_VERSION,
            digest: &digest,
            dedupe_key: &request.dedupe_key,
            run_delay_seconds: request.run_delay_seconds as f64,
            max_attempts: request.max_attempts,
        },
    )
    .await?;
    if result.status != LeaseStatus::Enqueued && result.status != LeaseStatus::Replayed {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("prepare"));
    }
    tx.execute(sql::NOTIFY, &[&request.project_id])
        .await
        .map_err(Error::sql)?;
    tx.commit().await.map_err(Error::sql)?;
    Ok(result)
}

async fn cancel_queued(client: &mut Client, dedupe: &str) -> Result<()> {
    let row = client
        .query_one(
            "SELECT id::text, generation FROM jobs.tasks \
             WHERE project_id = $1::text::uuid AND dedupe_key = $2 AND state = 'queued'",
            &[&PROOF_PROJECT, &dedupe],
        )
        .await
        .map_err(Error::sql)?;
    let task_id: String = row.try_get(0).map_err(Error::sql)?;
    let generation: i64 = row.try_get(1).map_err(Error::sql)?;
    if generation != 0 {
        return Err(Error::new("prepare"));
    }
    let tx = client.transaction().await.map_err(Error::sql)?;
    let result = lease::cancel(&tx, PROOF_PROJECT, &task_id, "canceller", 0).await?;
    tx.commit().await.map_err(Error::sql)?;
    if result.status != LeaseStatus::Cancelled {
        return Err(Error::new("prepare"));
    }
    Ok(())
}

async fn rollback_pair(client: &mut Client) -> Result<()> {
    let request = task("rolled-back", 1, 0, None, false, 3, 0);
    request.check()?;
    let payload = request.payload_text();
    let digest = request.digest();
    let tx = client.transaction().await.map_err(Error::sql)?;
    tx.execute(
        "INSERT INTO jw.intents (dedupe_key) VALUES ($1)",
        &[&request.dedupe_key],
    )
    .await
    .map_err(Error::sql)?;
    lease::enqueue(
        &tx,
        lease::Enqueue {
            project_id: &request.project_id,
            kind: KIND,
            payload_version: 1,
            payload: &payload,
            config_version: crate::api::CONFIG_VERSION,
            digest: &digest,
            dedupe_key: &request.dedupe_key,
            run_delay_seconds: 0.0,
            max_attempts: request.max_attempts,
        },
    )
    .await?;
    tx.rollback().await.map_err(Error::sql)?;
    let tasks: i64 = client
        .query_one(
            "SELECT count(*)::bigint FROM jobs.tasks \
             WHERE project_id = $1::text::uuid AND dedupe_key = 'rolled-back'",
            &[&PROOF_PROJECT],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    let intents: i64 = client
        .query_one(
            "SELECT count(*)::bigint FROM jw.intents WHERE dedupe_key = 'rolled-back'",
            &[],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if tasks != 0 || intents != 0 {
        return Err(Error::new("prepare"));
    }
    Ok(())
}
