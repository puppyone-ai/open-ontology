use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::watch;
use tokio_postgres::Client;

use crate::api::{LeaseStatus, Work, HEARTBEAT_SECONDS, KIND};
use crate::error::{Error, Result};
use crate::lease;
use crate::sql;

pub struct Gate {
    pub target: usize,
    pub prefix: Option<String>,
    pub arrived: AtomicUsize,
    pub release: watch::Sender<bool>,
}

pub struct WorkerIn {
    pub project_id: String,
    pub worker: String,
    pub stop: watch::Receiver<bool>,
    pub wake: watch::Receiver<u64>,
    pub gate: Arc<Gate>,
    pub gate_view: watch::Receiver<bool>,
}

pub async fn run_worker(client: &mut Client, input: WorkerIn) -> Result<()> {
    let kinds = vec![KIND.to_string()];
    let mut deadlocks = 0_u8;
    loop {
        if *input.stop.borrow() {
            return Ok(());
        }
        let tx = begin(client).await?;
        let claimed = match lease::claim(&tx, &input.project_id, &kinds, &input.worker).await {
            Ok(value) => value,
            Err(err) if err.is_deadlock() => {
                let _ = tx.rollback().await;
                deadlocks += 1;
                eprintln!("event=claim_deadlock worker={}", input.worker);
                if deadlocks > 3 {
                    return Err(err);
                }
                continue;
            }
            Err(err) => {
                let _ = tx.rollback().await;
                return Err(err);
            }
        };
        deadlocks = 0;
        if claimed.status != LeaseStatus::Claimed && claimed.status != LeaseStatus::None {
            tx.rollback().await.map_err(Error::sql)?;
            return Err(Error::new("claim"));
        }
        if claimed.status == LeaseStatus::None {
            tx.commit().await.map_err(Error::sql)?;
            wait_wake(&input).await;
            continue;
        }
        let task_id = claimed.task_id.clone().ok_or_else(|| Error::new("claim"))?;
        let generation = claimed.generation.ok_or_else(|| Error::new("claim"))?;
        let owner = claimed
            .lease_owner
            .clone()
            .ok_or_else(|| Error::new("claim"))?;
        let marked: String = tx
            .query_one(
                sql::INSERT_CLAIM,
                &[&task_id, &input.project_id, &owner, &generation],
            )
            .await
            .map_err(Error::sql)?
            .try_get(0)
            .map_err(Error::sql)?;
        if marked != task_id {
            tx.rollback().await.map_err(Error::sql)?;
            return Err(Error::new("claim"));
        }
        let dedupe: String = tx
            .query_one(sql::READ_DEDUPE, &[&task_id, &input.project_id])
            .await
            .map_err(Error::sql)?
            .try_get(0)
            .map_err(Error::sql)?;
        tx.commit().await.map_err(Error::sql)?;
        eprintln!(
            "event=claimed worker={} task={task_id} generation={generation}",
            input.worker
        );
        pass_gate(client, &input, &dedupe).await?;
        execute(client, &input, &task_id, generation, &owner).await?;
    }
}

async fn pass_gate(client: &mut Client, input: &WorkerIn, dedupe: &str) -> Result<()> {
    let Some(prefix) = &input.gate.prefix else {
        return Ok(());
    };
    if input.gate.target == 0 || !dedupe.starts_with(prefix.as_str()) {
        return Ok(());
    }
    let arrived = input.gate.arrived.fetch_add(1, Ordering::SeqCst) + 1;
    if arrived == input.gate.target {
        let running: i64 = client
            .query_one(
                "SELECT count(*)::bigint FROM jobs.tasks \
                 WHERE project_id = $1::text::uuid AND state = 'running' \
                   AND left(dedupe_key, char_length($2::text)) = $2::text",
                &[&input.project_id, prefix],
            )
            .await
            .map_err(Error::sql)?
            .get(0);
        client
            .execute(sql::OBSERVE, &[&"bulk_running", &running])
            .await
            .map_err(Error::sql)?;
        let _ = input.gate.release.send(true);
        eprintln!("event=barrier running={running}");
        return Ok(());
    }
    let mut view = input.gate_view.clone();
    let wait = view.wait_for(|open| *open);
    tokio::pin!(wait);
    tokio::select! {
        result = &mut wait => {
            result.map_err(|_| Error::new("barrier"))?;
        }
        _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
            eprintln!("event=barrier_timeout arrived={arrived}");
        }
    }
    Ok(())
}

async fn execute(
    client: &mut Client,
    input: &WorkerIn,
    task_id: &str,
    generation: i64,
    owner: &str,
) -> Result<()> {
    let tx = begin(client).await?;
    let row = tx
        .query_opt(
            sql::READ_WORK,
            &[&task_id, &input.project_id, &owner, &generation],
        )
        .await
        .map_err(Error::sql)?;
    let Some(row) = row else {
        tx.rollback().await.map_err(Error::sql)?;
        return Ok(());
    };
    let payload: String = row.try_get(0).map_err(Error::sql)?;
    let attempt: i32 = row.try_get(1).map_err(Error::sql)?;
    tx.commit().await.map_err(Error::sql)?;
    let work = match Work::parse(&payload) {
        Ok(work) => work,
        Err(_) => {
            finish_failure(client, input, task_id, owner, generation, "payload_rejected").await?;
            return Ok(());
        }
    };
    if let Ok(cap) = i32::try_from(work.fail_attempts)
        && cap > 0
        && attempt <= cap
    {
        finish_retry(client, input, task_id, owner, generation).await?;
        return Ok(());
    }
    if work.terminal_failure {
        finish_failure(client, input, task_id, owner, generation, "worker_failed").await?;
        return Ok(());
    }
    let Ok(target) = i32::try_from(work.units) else {
        finish_failure(client, input, task_id, owner, generation, "payload_rejected").await?;
        return Ok(());
    };
    loop {
        if *input.stop.borrow() {
            return Ok(());
        }
        let applied = applied(client, task_id, &input.project_id).await?;
        if generation == 1
            && let Some(stop_after) = work.stop_after
            && let Ok(stop) = i32::try_from(stop_after)
            && applied >= stop
        {
            hold(client, input, task_id, owner, generation).await?;
            return Ok(());
        }
        if applied >= target {
            finish_success(client, input, task_id, owner, generation).await?;
            return Ok(());
        }
        if heartbeat_if_due(client, input, task_id, owner, generation).await? {
            return Ok(());
        }
        let outcome = apply_effect(
            client,
            task_id,
            &input.project_id,
            owner,
            generation,
            applied + 1,
            target,
        )
        .await?;
        if outcome.0 == 0 {
            eprintln!(
                "event=lost worker={} task={task_id} generation={generation}",
                input.worker
            );
            return Ok(());
        }
    }
}

async fn hold(
    client: &mut Client,
    input: &WorkerIn,
    task_id: &str,
    owner: &str,
    generation: i64,
) -> Result<()> {
    eprintln!(
        "event=hold worker={} task={task_id} generation={generation}",
        input.worker
    );
    let mut stop = input.stop.clone();
    loop {
        if *stop.borrow() {
            return Ok(());
        }
        if heartbeat_if_due(client, input, task_id, owner, generation).await? {
            return Ok(());
        }
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    return Ok(());
                }
            }
        }
    }
}

async fn heartbeat_if_due(
    client: &mut Client,
    input: &WorkerIn,
    task_id: &str,
    owner: &str,
    generation: i64,
) -> Result<bool> {
    let due: bool = client
        .query_one(
            sql::HEARTBEAT_DUE,
            &[&task_id, &generation, &(HEARTBEAT_SECONDS as f64)],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if !due {
        return Ok(false);
    }
    let tx = begin(client).await?;
    let beat = lease::heartbeat(&tx, &input.project_id, task_id, owner, generation).await?;
    if beat.status != LeaseStatus::Extended {
        tx.rollback().await.map_err(Error::sql)?;
        eprintln!(
            "event=heartbeat status={} task={task_id} generation={generation}",
            beat.status.as_str()
        );
        return Ok(true);
    }
    let id: i64 = tx
        .query_one(
            sql::INSERT_BEAT,
            &[&task_id, &input.project_id, &owner, &generation],
        )
        .await
        .map_err(Error::sql)?
        .try_get(0)
        .map_err(Error::sql)?;
    tx.commit().await.map_err(Error::sql)?;
    eprintln!("event=heartbeat id={id} task={task_id} generation={generation}");
    Ok(false)
}

async fn apply_effect(
    client: &mut Client,
    task_id: &str,
    project_id: &str,
    owner: &str,
    generation: i64,
    seq: i32,
    target: i32,
) -> Result<(i64, i64)> {
    let tx = begin(client).await?;
    let row = tx
        .query_one(
            sql::APPLY_EFFECT,
            &[&task_id, &project_id, &owner, &generation, &seq, &target],
        )
        .await
        .map_err(Error::sql)?;
    let leased: i64 = row.try_get(0).map_err(Error::sql)?;
    let have: i64 = row.try_get(1).map_err(Error::sql)?;
    if leased == 0 {
        tx.rollback().await.map_err(Error::sql)?;
        return Ok((0, 0));
    }
    tx.commit().await.map_err(Error::sql)?;
    Ok((leased, have))
}

async fn finish_success(
    client: &mut Client,
    input: &WorkerIn,
    task_id: &str,
    owner: &str,
    generation: i64,
) -> Result<()> {
    let tx = begin(client).await?;
    let result =
        lease::complete_success(&tx, &input.project_id, task_id, owner, generation).await?;
    tx.commit().await.map_err(Error::sql)?;
    eprintln!(
        "event=complete status={} task={task_id} generation={generation}",
        result.status.as_str()
    );
    Ok(())
}

async fn finish_failure(
    client: &mut Client,
    input: &WorkerIn,
    task_id: &str,
    owner: &str,
    generation: i64,
    error_kind: &str,
) -> Result<()> {
    let tx = begin(client).await?;
    let result = lease::complete_failure(
        &tx,
        &input.project_id,
        task_id,
        owner,
        generation,
        error_kind,
    )
    .await?;
    tx.commit().await.map_err(Error::sql)?;
    eprintln!(
        "event=failed status={} task={task_id} generation={generation}",
        result.status.as_str()
    );
    Ok(())
}

async fn finish_retry(
    client: &mut Client,
    input: &WorkerIn,
    task_id: &str,
    owner: &str,
    generation: i64,
) -> Result<()> {
    let tx = begin(client).await?;
    let result = lease::retry_failure(
        &tx,
        &input.project_id,
        task_id,
        owner,
        generation,
        "worker_failed",
    )
    .await?;
    tx.commit().await.map_err(Error::sql)?;
    eprintln!(
        "event=retry status={} task={task_id} generation={generation}",
        result.status.as_str()
    );
    Ok(())
}

async fn applied(client: &Client, task_id: &str, project_id: &str) -> Result<i32> {
    let value: i32 = client
        .query_one(sql::READ_APPLIED, &[&task_id, &project_id])
        .await
        .map_err(Error::sql)?
        .get(0);
    Ok(value)
}

async fn begin(client: &mut Client) -> Result<tokio_postgres::Transaction<'_>> {
    let tx = client.transaction().await.map_err(Error::sql)?;
    tx.batch_execute("SET LOCAL statement_timeout = '10s'")
        .await
        .map_err(Error::sql)?;
    Ok(tx)
}

async fn wait_wake(input: &WorkerIn) {
    let mut wake = input.wake.clone();
    let mut stop = input.stop.clone();
    tokio::select! {
        _ = wake.changed() => {}
        _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
        _ = stop.changed() => {}
    }
}

pub async fn observe(client: &Client, name: &str, value: i64) -> Result<()> {
    if !(1..=64).contains(&name.len())
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
    {
        return Err(Error::new("input"));
    }
    client
        .execute(sql::OBSERVE, &[&name, &value])
        .await
        .map_err(Error::sql)?;
    Ok(())
}
