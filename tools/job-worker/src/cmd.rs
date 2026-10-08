use tokio_postgres::Client;

use crate::api::{LeaseStatus, PROOF_PROJECT};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::lease;
use crate::session;
use crate::sql;
use crate::worker;

pub enum WaitSpec {
    Observation { name: String, value: i64 },
    State { dedupe: String, state: String },
    Applied { dedupe: String, at_least: i32 },
    Beat { dedupe: String },
    Expired { dedupe: String },
    Schedule { state: String },
}

pub async fn wait(config: &Config, spec: WaitSpec, seconds: i64) -> Result<()> {
    if !(1..=600).contains(&seconds) {
        return Err(Error::new("input"));
    }
    let client = session::runtime(config).await?;
    let start: i64 = client
        .query_one(
            "SELECT floor(extract(epoch FROM clock_timestamp()))::bigint",
            &[],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    loop {
        if ready(&client, &spec).await? {
            println!("wait=ok");
            return Ok(());
        }
        let now: i64 = client
            .query_one(
                "SELECT floor(extract(epoch FROM clock_timestamp()))::bigint",
                &[],
            )
            .await
            .map_err(Error::sql)?
            .get(0);
        if now >= start + seconds {
            return Err(Error::new("timeout"));
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

async fn ready(client: &Client, spec: &WaitSpec) -> Result<bool> {
    match spec {
        WaitSpec::Observation { name, value } => {
            check_name(name)?;
            let row = client
                .query_opt(
                    "SELECT value FROM jw.observations WHERE name = $1",
                    &[&name.as_str()],
                )
                .await
                .map_err(Error::sql)?;
            Ok(row.is_some_and(|row| row.get::<_, i64>(0) == *value))
        }
        WaitSpec::State { dedupe, state } => {
            check_dedupe(dedupe)?;
            check_state(state)?;
            let row = client
                .query_opt(
                    "SELECT state FROM jobs.tasks WHERE project_id = $1::text::uuid AND dedupe_key = $2",
                    &[&PROOF_PROJECT, &dedupe.as_str()],
                )
                .await
                .map_err(Error::sql)?;
            Ok(row.is_some_and(|row| row.get::<_, String>(0) == *state))
        }
        WaitSpec::Applied { dedupe, at_least } => {
            check_dedupe(dedupe)?;
            let row = client
                .query_opt(
                    "SELECT COALESCE(c.applied, 0) \
                     FROM jobs.tasks t \
                     LEFT JOIN jw.checkpoints c ON c.task_id = t.id \
                     WHERE t.project_id = $1::text::uuid AND t.dedupe_key = $2",
                    &[&PROOF_PROJECT, &dedupe.as_str()],
                )
                .await
                .map_err(Error::sql)?;
            Ok(row.is_some_and(|row| row.get::<_, i32>(0) >= *at_least))
        }
        WaitSpec::Beat { dedupe } => {
            check_dedupe(dedupe)?;
            let row = client
                .query_opt(
                    "SELECT floor(extract(epoch FROM (min(b.observed_at) - min(c.claimed_at))))::bigint \
                     FROM jw.claims c \
                     JOIN jobs.tasks t ON t.id = c.task_id \
                     JOIN jw.beats b ON b.task_id = c.task_id AND b.generation = c.generation \
                     WHERE t.project_id = $1::text::uuid AND t.dedupe_key = $2 AND c.generation = 1",
                    &[&PROOF_PROJECT, &dedupe.as_str()],
                )
                .await
                .map_err(Error::sql)?;
            Ok(row.and_then(|row| row.get::<_, Option<i64>>(0)).is_some_and(|gap| gap >= 10))
        }
        WaitSpec::Expired { dedupe } => {
            check_dedupe(dedupe)?;
            let row = client
                .query_opt(
                    "SELECT (state = 'running' AND lease_until <= clock_timestamp()) \
                     FROM jobs.tasks WHERE project_id = $1::text::uuid AND dedupe_key = $2",
                    &[&PROOF_PROJECT, &dedupe.as_str()],
                )
                .await
                .map_err(Error::sql)?;
            Ok(row.is_some_and(|row| row.get::<_, bool>(0)))
        }
        WaitSpec::Schedule { state } => {
            check_state(state)?;
            let row = client
                .query_opt(
                    "SELECT t.state \
                     FROM jobs.tasks t \
                     JOIN jobs.schedule_occurrences o ON o.task_id = t.id \
                     JOIN jobs.schedules s ON s.id = o.schedule_id \
                     WHERE s.project_id = $1::text::uuid AND s.dedupe_prefix = 'once-side-effect'",
                    &[&PROOF_PROJECT],
                )
                .await
                .map_err(Error::sql)?;
            Ok(row.is_some_and(|row| row.get::<_, String>(0) == *state))
        }
    }
}

pub async fn show_lease(config: &Config, dedupe: &str) -> Result<()> {
    check_dedupe(dedupe)?;
    let client = session::runtime(config).await?;
    let row = client
        .query_opt(
            "SELECT id::text, state, COALESCE(lease_owner, ''), generation, \
                    COALESCE(lease_until::text, ''), attempt \
             FROM jobs.tasks WHERE project_id = $1::text::uuid AND dedupe_key = $2",
            &[&PROOF_PROJECT, &dedupe],
        )
        .await
        .map_err(Error::sql)?;
    let Some(row) = row else {
        return Err(Error::new("missing"));
    };
    let task_id: String = row.try_get(0).map_err(Error::sql)?;
    let state: String = row.try_get(1).map_err(Error::sql)?;
    let owner: String = row.try_get(2).map_err(Error::sql)?;
    let generation: i64 = row.try_get(3).map_err(Error::sql)?;
    let until: String = row.try_get(4).map_err(Error::sql)?;
    let attempt: i32 = row.try_get(5).map_err(Error::sql)?;
    println!("dedupe={dedupe}");
    println!("task_id={task_id}");
    println!("state={state}");
    println!("owner={owner}");
    println!("generation={generation}");
    println!("lease_until={until}");
    println!("attempt={attempt}");
    Ok(())
}

pub async fn cancel_dedupe(config: &Config, dedupe: &str) -> Result<()> {
    check_dedupe(dedupe)?;
    let mut client = session::runtime(config).await?;
    let row = client
        .query_one(
            "SELECT id::text, state, generation, COALESCE(lease_owner, '') \
             FROM jobs.tasks WHERE project_id = $1::text::uuid AND dedupe_key = $2",
            &[&PROOF_PROJECT, &dedupe],
        )
        .await
        .map_err(Error::sql)?;
    let task_id: String = row.try_get(0).map_err(Error::sql)?;
    let state: String = row.try_get(1).map_err(Error::sql)?;
    let generation: i64 = row.try_get(2).map_err(Error::sql)?;
    let owner: String = row.try_get(3).map_err(Error::sql)?;
    let (worker, held_generation) = if state == "queued" {
        ("canceller", 0_i64)
    } else if state == "running" {
        if owner.is_empty() {
            return Err(Error::new("cancel"));
        }
        (owner.as_str(), generation)
    } else {
        return Err(Error::new("cancel"));
    };
    let mut result = None;
    for _ in 0..3 {
        let tx = client.transaction().await.map_err(Error::sql)?;
        match lease::cancel(&tx, PROOF_PROJECT, &task_id, worker, held_generation).await {
            Ok(value) => {
                tx.commit().await.map_err(Error::sql)?;
                result = Some(value);
                break;
            }
            Err(err) if err.is_deadlock() => {
                let _ = tx.rollback().await;
            }
            Err(err) => {
                let _ = tx.rollback().await;
                return Err(err);
            }
        }
    }
    let Some(result) = result else {
        return Err(Error::new("cancel"));
    };
    println!("cancel={}", result.status.as_str());
    if result.status != LeaseStatus::Cancelled {
        return Err(Error::new("cancel"));
    }
    if state == "running" {
        let tx = client.transaction().await.map_err(Error::sql)?;
        let follow =
            lease::complete_success(&tx, PROOF_PROJECT, &task_id, worker, held_generation).await?;
        tx.commit().await.map_err(Error::sql)?;
        println!("follow={}", follow.status.as_str());
        let terminal = i64::from(follow.status == LeaseStatus::Terminal);
        let succeeded = i64::from(follow.status == LeaseStatus::Succeeded);
        worker::observe(&client, "cancel_follow_terminal", terminal).await?;
        worker::observe(&client, "cancel_follow_succeeded", succeeded).await?;
    }
    Ok(())
}

pub async fn fence(config: &Config, phase: &str, dedupe: &str) -> Result<()> {
    if phase != "before" && phase != "after" {
        return Err(Error::new("input"));
    }
    check_dedupe(dedupe)?;
    let mut client = session::runtime(config).await?;
    let row = client
        .query_one(
            "SELECT t.id::text, t.state, c.owner, c.generation, \
                    (t.lease_until IS NOT NULL AND t.lease_until <= clock_timestamp()) \
             FROM jobs.tasks t \
             JOIN jw.claims c ON c.task_id = t.id AND c.generation = 1 \
             WHERE t.project_id = $1::text::uuid AND t.dedupe_key = $2",
            &[&PROOF_PROJECT, &dedupe],
        )
        .await
        .map_err(Error::sql)?;
    let task_id: String = row.try_get(0).map_err(Error::sql)?;
    let state: String = row.try_get(1).map_err(Error::sql)?;
    let owner: String = row.try_get(2).map_err(Error::sql)?;
    let generation: i64 = row.try_get(3).map_err(Error::sql)?;
    let expired: bool = row.try_get(4).map_err(Error::sql)?;
    if phase == "before" && !expired {
        return Err(Error::new("fence"));
    }
    if phase == "after" && state != "succeeded" {
        return Err(Error::new("fence"));
    }
    let tx = client.transaction().await.map_err(Error::sql)?;
    let beat = lease::heartbeat(&tx, PROOF_PROJECT, &task_id, &owner, generation).await?;
    tx.commit().await.map_err(Error::sql)?;
    let tx = client.transaction().await.map_err(Error::sql)?;
    let done = lease::complete_success(&tx, PROOF_PROJECT, &task_id, &owner, generation).await?;
    tx.commit().await.map_err(Error::sql)?;
    let tx = client.transaction().await.map_err(Error::sql)?;
    let effect = tx
        .query_one(
            sql::FENCE_EFFECT,
            &[&task_id, &PROOF_PROJECT, &owner, &generation],
        )
        .await
        .map_err(Error::sql)?;
    let inserted: i64 = effect.try_get(1).map_err(Error::sql)?;
    tx.commit().await.map_err(Error::sql)?;
    let prefix = format!("fence_{phase}");
    worker::observe(&client, &format!("{prefix}_heartbeat"), status_code(beat.status)).await?;
    worker::observe(&client, &format!("{prefix}_complete"), status_code(done.status)).await?;
    worker::observe(&client, &format!("{prefix}_inserted"), inserted).await?;
    println!(
        "phase={phase} heartbeat={} complete={} inserted={inserted}",
        beat.status.as_str(),
        done.status.as_str()
    );
    Ok(())
}

fn status_code(status: LeaseStatus) -> i64 {
    match status {
        LeaseStatus::LostLease => 1,
        LeaseStatus::Terminal => 2,
        LeaseStatus::Succeeded => 3,
        LeaseStatus::Extended => 4,
        _ => 0,
    }
}

fn check_name(name: &str) -> Result<()> {
    if !(1..=64).contains(&name.len())
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
    {
        return Err(Error::new("input"));
    }
    Ok(())
}

fn check_dedupe(dedupe: &str) -> Result<()> {
    if !(1..=256).contains(&dedupe.len()) || !dedupe.is_ascii() {
        return Err(Error::new("input"));
    }
    Ok(())
}

fn check_state(state: &str) -> Result<()> {
    match state {
        "queued" | "running" | "succeeded" | "failed" | "cancelled" => Ok(()),
        _ => Err(Error::new("input")),
    }
}
