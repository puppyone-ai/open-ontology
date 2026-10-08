use tokio_postgres::Client;

use crate::api::{BOOT_SHA, CONTROL_SHA, EVENT_SHA, LEASE_SHA, ADAPT_SHA, PROOF_PROJECT};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::session;
use crate::sql;

pub async fn report(config: &Config) -> Result<String> {
    let client = session::runtime(config).await?;
    let saw = load_obs(&client).await?;
    let cohort = load_cohort(&client).await?;
    let proof = load_proof(&client, saw.xact_at_prepare).await?;
    let body = report_body(&saw, &cohort, &proof);
    serde_json::to_string_pretty(&body).map_err(|_| Error::new("report"))
}

struct Saw {
    bulk_running_peak: Option<i64>,
    replayed: Option<i64>,
    notify_left_queued: Option<i64>,
    xact_at_prepare: Option<i64>,
    cancel_follow_terminal: Option<i64>,
    cancel_follow_succeeded: Option<i64>,
    fence_before_complete: Option<i64>,
    fence_before_heartbeat: Option<i64>,
    fence_before_inserted: Option<i64>,
    fence_after_complete: Option<i64>,
    fence_after_heartbeat: Option<i64>,
    fence_after_inserted: Option<i64>,
}

async fn load_obs(client: &Client) -> Result<Saw> {
    Ok(Saw {
        bulk_running_peak: obs(client, "bulk_running").await?,
        replayed: obs(client, "replayed").await?,
        notify_left_queued: obs(client, "notify_left_queued").await?,
        xact_at_prepare: obs(client, "xact_at_prepare").await?,
        cancel_follow_terminal: obs(client, "cancel_follow_terminal").await?,
        cancel_follow_succeeded: obs(client, "cancel_follow_succeeded").await?,
        fence_before_complete: obs(client, "fence_before_complete").await?,
        fence_before_heartbeat: obs(client, "fence_before_heartbeat").await?,
        fence_before_inserted: obs(client, "fence_before_inserted").await?,
        fence_after_complete: obs(client, "fence_after_complete").await?,
        fence_after_heartbeat: obs(client, "fence_after_heartbeat").await?,
        fence_after_inserted: obs(client, "fence_after_inserted").await?,
    })
}

struct Cohort {
    bulk_succeeded: i64,
    bulk_effects: i64,
    bulk_owners: i64,
    checkpoint: TaskView,
    checkpoint_generations: i64,
    hold: TaskView,
    retry: TaskView,
    dead: TaskView,
    terminal: TaskView,
    bad: TaskView,
    notify: TaskView,
    queued: TaskView,
    dead_letters: i64,
}

async fn load_cohort(client: &Client) -> Result<Cohort> {
    Ok(Cohort {
        bulk_succeeded: count_state(client, "bulk-%", "succeeded").await?,
        bulk_effects: effects_like(client, "bulk-%").await?,
        bulk_owners: bulk_owners(client).await?,
        checkpoint: task_view(client, "checkpoint").await?,
        checkpoint_generations: checkpoint_generations(client).await?,
        hold: task_view(client, "hold-cancel").await?,
        retry: task_view(client, "retry-once").await?,
        dead: task_view(client, "dead-letter").await?,
        terminal: task_view(client, "terminal-fail").await?,
        bad: task_view(client, "bad-payload").await?,
        notify: task_view(client, "notify-loss").await?,
        queued: task_view(client, "queued-cancel").await?,
        dead_letters: count_value(
            client,
            "SELECT count(*)::bigint FROM jobs.dead_letters \
             WHERE project_id = $1::text::uuid AND dedupe_key = 'dead-letter'",
        )
        .await?,
    })
}

struct Proof {
    schedule_occurrences: i64,
    schedule_tasks: i64,
    schedule_effects: i64,
    rollback_tasks: i64,
    rollback_intents: i64,
    effect_rows: i64,
    effect_txids: i64,
    lease_span: Option<i64>,
    beat_gap: Option<i64>,
    beat_lease: Option<i64>,
    target_fence_true: i64,
    duplicate_succeeded_events: i64,
    checksums_match: bool,
    xact_commit_delta: Option<i64>,
}

async fn load_proof(client: &Client, xact_at_prepare: Option<i64>) -> Result<Proof> {
    let xact_now = scalar(client, sql::XACT_COMMIT).await?;
    let schedule_occurrences = count_value(
        client,
        "SELECT count(*)::bigint \
         FROM jobs.schedule_occurrences o \
         JOIN jobs.schedules s ON s.id = o.schedule_id \
         WHERE s.project_id = $1::text::uuid AND s.dedupe_prefix = 'once-side-effect'",
    )
    .await?;
    let schedule_tasks = count_value(
        client,
        "SELECT count(*)::bigint \
         FROM jobs.tasks t \
         JOIN jobs.schedule_occurrences o ON o.task_id = t.id \
         JOIN jobs.schedules s ON s.id = o.schedule_id \
         WHERE s.project_id = $1::text::uuid AND s.dedupe_prefix = 'once-side-effect'",
    )
    .await?;
    let schedule_effects = count_value(
        client,
        "SELECT count(*)::bigint \
         FROM jw.effects e \
         JOIN jobs.schedule_occurrences o ON o.task_id = e.task_id \
         JOIN jobs.schedules s ON s.id = o.schedule_id \
         WHERE s.project_id = $1::text::uuid AND s.dedupe_prefix = 'once-side-effect'",
    )
    .await?;
    let rollback_tasks = count_value(
        client,
        "SELECT count(*)::bigint FROM jobs.tasks \
         WHERE project_id = $1::text::uuid AND dedupe_key = 'rolled-back'",
    )
    .await?;
    let rollback_intents = scalar(
        client,
        "SELECT count(*)::bigint FROM jw.intents WHERE dedupe_key = 'rolled-back'",
    )
    .await?;
    let counts = client
        .query_one(
            "SELECT count(*)::bigint, count(DISTINCT txid)::bigint FROM jw.effects",
            &[],
        )
        .await
        .map_err(Error::sql)?;
    let beat = first_beat(client).await?;
    Ok(Proof {
        schedule_occurrences,
        schedule_tasks,
        schedule_effects,
        rollback_tasks,
        rollback_intents,
        effect_rows: counts.try_get(0).map_err(Error::sql)?,
        effect_txids: counts.try_get(1).map_err(Error::sql)?,
        lease_span: measure(
            client,
            "SELECT floor(extract(epoch FROM (c.lease_until - c.claimed_at)))::bigint \
             FROM jw.claims c \
             JOIN jobs.tasks t ON t.id = c.task_id \
             WHERE t.project_id = $1::text::uuid AND t.dedupe_key = 'checkpoint' AND c.generation = 1",
        )
        .await?,
        beat_gap: beat.0,
        beat_lease: beat.1,
        target_fence_true: scalar(
            client,
            "SELECT count(*)::bigint FROM jobs.leases WHERE target_fence_certified",
        )
        .await?,
        duplicate_succeeded_events: count_value(
            client,
            "SELECT count(*)::bigint FROM ( \
               SELECT task_id FROM jobs.task_events \
               WHERE project_id = $1::text::uuid AND to_state = 'succeeded' \
               GROUP BY task_id HAVING count(*) > 1 \
             ) d",
        )
        .await?,
        checksums_match: checksums(client).await?,
        xact_commit_delta: xact_at_prepare.map(|then| xact_now - then),
    })
}

fn report_body(saw: &Saw, cohort: &Cohort, proof: &Proof) -> serde_json::Value {
    serde_json::json!({
        "bulk_running_peak": num(saw.bulk_running_peak),
        "bulk_succeeded": cohort.bulk_succeeded,
        "bulk_effects": cohort.bulk_effects,
        "bulk_owners": cohort.bulk_owners,
        "replayed": num(saw.replayed),
        "rollback_tasks": proof.rollback_tasks,
        "rollback_intents": proof.rollback_intents,
        "notify_left_queued": num(saw.notify_left_queued),
        "notify_loss_state": cohort.notify.state,
        "notify_loss_effects": cohort.notify.effects,
        "schedule_occurrences": proof.schedule_occurrences,
        "schedule_tasks": proof.schedule_tasks,
        "schedule_effects": proof.schedule_effects,
        "retry_state": cohort.retry.state,
        "retry_effects": cohort.retry.effects,
        "retry_attempt": cohort.retry.attempt,
        "dead_state": cohort.dead.state,
        "dead_error": cohort.dead.last_error,
        "dead_effects": cohort.dead.effects,
        "dead_letters": cohort.dead_letters,
        "terminal_state": cohort.terminal.state,
        "terminal_error": cohort.terminal.last_error,
        "terminal_effects": cohort.terminal.effects,
        "bad_state": cohort.bad.state,
        "bad_error": cohort.bad.last_error,
        "bad_effects": cohort.bad.effects,
        "queued_cancel_state": cohort.queued.state,
        "queued_cancel_effects": cohort.queued.effects,
        "running_cancel_state": cohort.hold.state,
        "running_cancel_effects": cohort.hold.effects,
        "cancel_follow_terminal": num(saw.cancel_follow_terminal),
        "cancel_follow_succeeded": num(saw.cancel_follow_succeeded),
        "checkpoint_state": cohort.checkpoint.state,
        "checkpoint_effects": cohort.checkpoint.effects,
        "checkpoint_generations": cohort.checkpoint_generations,
        "fence_before_complete": code_name(saw.fence_before_complete),
        "fence_before_heartbeat": code_name(saw.fence_before_heartbeat),
        "fence_before_inserted": num(saw.fence_before_inserted),
        "fence_after_complete": code_name(saw.fence_after_complete),
        "fence_after_heartbeat": code_name(saw.fence_after_heartbeat),
        "fence_after_inserted": num(saw.fence_after_inserted),
        "lease_span_seconds": num(proof.lease_span),
        "beat_gap_seconds": num(proof.beat_gap),
        "beat_lease_seconds": num(proof.beat_lease),
        "effect_rows": proof.effect_rows,
        "effect_txids": proof.effect_txids,
        "xact_commit_delta": num(proof.xact_commit_delta),
        "target_fence_true": proof.target_fence_true,
        "duplicate_succeeded_events": proof.duplicate_succeeded_events,
        "checksums_match": proof.checksums_match,
    })
}

async fn bulk_owners(client: &Client) -> Result<i64> {
    count_value(
        client,
        "SELECT count(DISTINCT c.owner)::bigint \
         FROM jw.claims c \
         JOIN jobs.tasks t ON t.id = c.task_id \
         WHERE t.project_id = $1::text::uuid AND t.dedupe_key LIKE 'bulk-%'",
    )
    .await
}

async fn checkpoint_generations(client: &Client) -> Result<i64> {
    count_value(
        client,
        "SELECT count(DISTINCT e.generation)::bigint \
         FROM jw.effects e \
         JOIN jobs.tasks t ON t.id = e.task_id \
         WHERE t.project_id = $1::text::uuid AND t.dedupe_key = 'checkpoint'",
    )
    .await
}

async fn count_value(client: &Client, statement: &str) -> Result<i64> {
    let value: i64 = client
        .query_one(statement, &[&PROOF_PROJECT])
        .await
        .map_err(Error::sql)?
        .get(0);
    Ok(value)
}

async fn scalar(client: &Client, statement: &str) -> Result<i64> {
    let value: i64 = client
        .query_one(statement, &[])
        .await
        .map_err(Error::sql)?
        .get(0);
    Ok(value)
}

async fn first_beat(client: &Client) -> Result<(Option<i64>, Option<i64>)> {
    let beat = client
        .query_opt(
            "SELECT floor(extract(epoch FROM (b.observed_at - c.claimed_at)))::bigint, \
                    floor(extract(epoch FROM (b.lease_until - b.observed_at)))::bigint \
             FROM jw.beats b \
             JOIN jw.claims c ON c.task_id = b.task_id AND c.generation = b.generation \
             JOIN jobs.tasks t ON t.id = b.task_id \
             WHERE t.project_id = $1::text::uuid AND t.dedupe_key = 'checkpoint' AND b.generation = 1 \
             ORDER BY b.observed_at \
             LIMIT 1",
            &[&PROOF_PROJECT],
        )
        .await
        .map_err(Error::sql)?;
    Ok(match beat {
        Some(row) => (row.get::<_, Option<i64>>(0), row.get::<_, Option<i64>>(1)),
        None => (None, None),
    })
}

struct TaskView {
    state: String,
    attempt: i32,
    last_error: Option<String>,
    effects: i64,
}

async fn task_view(client: &Client, dedupe: &str) -> Result<TaskView> {
    let row = client
        .query_one(
            "SELECT t.state, t.attempt, t.last_error, \
                    (SELECT count(*)::bigint FROM jw.effects e WHERE e.task_id = t.id) \
             FROM jobs.tasks t \
             WHERE t.project_id = $1::text::uuid AND t.dedupe_key = $2",
            &[&PROOF_PROJECT, &dedupe],
        )
        .await
        .map_err(Error::sql)?;
    Ok(TaskView {
        state: row.try_get(0).map_err(Error::sql)?,
        attempt: row.try_get(1).map_err(Error::sql)?,
        last_error: row.try_get(2).map_err(Error::sql)?,
        effects: row.try_get(3).map_err(Error::sql)?,
    })
}

async fn count_state(client: &Client, like: &str, state: &str) -> Result<i64> {
    let value: i64 = client
        .query_one(
            "SELECT count(*)::bigint FROM jobs.tasks \
             WHERE project_id = $1::text::uuid AND dedupe_key LIKE $2 AND state = $3",
            &[&PROOF_PROJECT, &like, &state],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    Ok(value)
}

async fn effects_like(client: &Client, like: &str) -> Result<i64> {
    let value: i64 = client
        .query_one(
            "SELECT count(*)::bigint FROM jw.effects e \
             JOIN jobs.tasks t ON t.id = e.task_id \
             WHERE t.project_id = $1::text::uuid AND t.dedupe_key LIKE $2",
            &[&PROOF_PROJECT, &like],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    Ok(value)
}

async fn obs(client: &Client, name: &str) -> Result<Option<i64>> {
    let row = client
        .query_opt("SELECT value FROM jw.observations WHERE name = $1", &[&name])
        .await
        .map_err(Error::sql)?;
    Ok(row.map(|row| row.get(0)))
}

async fn measure(client: &Client, statement: &str) -> Result<Option<i64>> {
    let row = client
        .query_opt(statement, &[&PROOF_PROJECT])
        .await
        .map_err(Error::sql)?;
    Ok(row.and_then(|row| row.get(0)))
}

async fn checksums(client: &Client) -> Result<bool> {
    let rows = client
        .query("SELECT version, checksum FROM jobs.schema_migrations", &[])
        .await
        .map_err(Error::sql)?;
    let mut seen = std::collections::BTreeMap::new();
    for row in rows {
        let version: String = row.try_get(0).map_err(Error::sql)?;
        let checksum: String = row.try_get(1).map_err(Error::sql)?;
        seen.insert(version, checksum);
    }
    let adapt: String = client
        .query_one("SELECT checksum FROM jw.install WHERE id = 1", &[])
        .await
        .map_err(Error::sql)?
        .get(0);
    let expected = [
        ("jobs-bootstrap/1", BOOT_SHA),
        ("jobs-control/1", CONTROL_SHA),
        ("jobs-event-project/1", EVENT_SHA),
        ("jobs-lease-operations/1", LEASE_SHA),
    ];
    let migrations_ok = expected
        .iter()
        .all(|(version, sha)| seen.get(*version).map(String::as_str) == Some(*sha));
    Ok(migrations_ok && adapt == ADAPT_SHA)
}

fn num(value: Option<i64>) -> serde_json::Value {
    match value {
        Some(value) => serde_json::json!(value),
        None => serde_json::Value::Null,
    }
}

fn code_name(value: Option<i64>) -> serde_json::Value {
    match value {
        Some(1) => serde_json::json!("lost_lease"),
        Some(2) => serde_json::json!("terminal"),
        Some(3) => serde_json::json!("succeeded"),
        Some(4) => serde_json::json!("extended"),
        Some(_) => serde_json::json!("other"),
        None => serde_json::Value::Null,
    }
}
