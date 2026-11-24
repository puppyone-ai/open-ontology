//! Worker pool over the job functions (10 §4.1.1, §4.1.7).
//!
//! One claimer task per job kind. A claimer takes a semaphore permit first
//! and only then calls `claim`, so the `queued` rows in the database are the
//! queue and nothing buffers in memory. Each claimed job runs in its own task
//! next to a heartbeat task (every [`HEARTBEAT_SECONDS`]); the heartbeat
//! mirrors `cancel_requested` and a lost lease into flags the handler polls.
//! A handler returns an [`Outcome`]; the scheduler turns it into
//! `complete`, `retry`, `fail` or `ack_cancel`. A handler that wants its own
//! writes and the completion in one transaction (4.1.3) calls
//! `store::jobs::complete` itself and returns [`Outcome::Completed`].
//!
//! Formal: FM1. Every completion goes through the `(owner, generation)` CAS in
//! the SQL functions; after `lost_lease` the result is discarded, not retried.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::{Notify, Semaphore, watch};
use tokio::task::JoinHandle;

use super::{FailureClass, HEARTBEAT_SECONDS, Job, JobKind, JobStatus, report_failure};
use crate::store::jobs as store;

/// What a handler tells the scheduler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The handler already completed the job (usually inside its own commit).
    Completed,
    /// Success; the scheduler completes the job.
    Done,
    /// Failure; the scheduler applies the retry policy for `class`.
    Failed {
        /// Classification (4.1.4, 4.1.6).
        class: FailureClass,
        /// `[A-Za-z0-9_.:-]{1,96}`, stored in `job.last_error`.
        code: String,
    },
    /// The handler saw `cancel_requested` and stopped.
    Cancelled,
}

impl Outcome {
    /// Failure helper.
    pub fn failed(class: FailureClass, code: &str) -> Self {
        Self::Failed {
            class,
            code: code.to_owned(),
        }
    }
}

/// Everything a handler may use for one leased job.
#[derive(Clone)]
pub struct JobCtx {
    /// The leased row (as returned by `claim`).
    pub job: Job,
    /// Worker id holding the lease.
    pub owner: String,
    /// Pool for the handler's own queries.
    pub pool: PgPool,
    cancel: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
}

impl JobCtx {
    /// Context without a scheduler (tests, direct calls). Flags start clear.
    pub fn detached(job: Job, owner: &str, pool: PgPool) -> Self {
        Self {
            job,
            owner: owner.to_owned(),
            pool,
            cancel: Arc::new(AtomicBool::new(false)),
            lost: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A cancel was requested (seen by the last heartbeat).
    pub fn cancel_requested(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// The lease is gone; discard local results.
    pub fn lease_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }

    /// Either of the above: stop at the next checkpoint.
    pub fn should_stop(&self) -> bool {
        self.cancel_requested() || self.lease_lost()
    }

    /// The cancel flag as a shared handle, for a blocking task that outlives the borrow.
    pub fn cancel_flag_arc(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    /// The cancel flag itself, for code that polls an `&AtomicBool`.
    pub fn cancel_flag(&self) -> &AtomicBool {
        &self.cancel
    }

    /// Flag setters for tests.
    pub fn set_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Boxed handler future.
pub type HandlerFuture<'a> = Pin<Box<dyn Future<Output = Outcome> + Send + 'a>>;

/// A stage implementation for one job kind.
pub trait JobHandler: Send + Sync + 'static {
    /// Run one leased job.
    fn run(&self, ctx: JobCtx) -> HandlerFuture<'_>;
}

/// Scheduler settings.
#[derive(Clone, Debug)]
pub struct SchedulerConfig {
    /// Worker id, `<hostname>/<pid>/<nonce>`.
    pub owner: String,
    /// Concurrent jobs per kind (`concurrency.*`, 2.8); kinds without a
    /// handler are not claimed.
    pub concurrency: HashMap<JobKind, usize>,
    /// Sleep between empty claims.
    pub poll_interval: Duration,
    /// Heartbeat period.
    pub heartbeat: Duration,
}

impl SchedulerConfig {
    /// Defaults: 2 jobs per kind, 500 ms poll, 10 s heartbeat.
    pub fn new(owner: &str) -> Self {
        Self {
            owner: owner.to_owned(),
            concurrency: HashMap::new(),
            poll_interval: Duration::from_millis(500),
            heartbeat: Duration::from_secs(HEARTBEAT_SECONDS),
        }
    }
}

/// Claimers for a set of handlers.
pub struct Scheduler {
    pool: PgPool,
    cfg: SchedulerConfig,
    handlers: HashMap<JobKind, Arc<dyn JobHandler>>,
}

/// Running scheduler.
pub struct SchedulerHandle {
    stop: watch::Sender<bool>,
    wake: Arc<Notify>,
    tasks: Vec<JoinHandle<()>>,
}

impl SchedulerHandle {
    /// Wake the claimers now (after an enqueue).
    pub fn wake(&self) {
        self.wake.notify_waiters();
    }

    /// Stop claiming, let running jobs reach their next checkpoint and
    /// return. Leases of anything still running expire on their own.
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        self.wake.notify_waiters();
        for t in self.tasks {
            let _ = t.await;
        }
    }
}

impl Scheduler {
    /// New scheduler.
    pub fn new(
        pool: PgPool,
        cfg: SchedulerConfig,
        handlers: HashMap<JobKind, Arc<dyn JobHandler>>,
    ) -> Self {
        Self {
            pool,
            cfg,
            handlers,
        }
    }

    /// Start one claimer per handler kind.
    pub fn spawn(self) -> SchedulerHandle {
        let (stop_tx, stop_rx) = watch::channel(false);
        let wake = Arc::new(Notify::new());
        let mut tasks = Vec::new();
        let mut semaphores = slot_pools(&self.cfg, self.handlers.keys().copied());
        for (kind, handler) in self.handlers {
            tasks.push(tokio::spawn(claim_loop(
                self.pool.clone(),
                kind,
                handler,
                self.cfg.clone(),
                semaphores
                    .remove(&kind)
                    .unwrap_or_else(|| Arc::new(Semaphore::new(1))),
                stop_rx.clone(),
                wake.clone(),
            )));
        }
        SchedulerHandle {
            stop: stop_tx,
            wake,
            tasks,
        }
    }
}

/// Slots of one job kind: `concurrency.*`, 2 when unset, never 0.
pub fn slots_for(cfg: &SchedulerConfig, kind: JobKind) -> usize {
    cfg.concurrency.get(&kind).copied().unwrap_or(2).max(1)
}

/// One semaphore per kind. Every kind has its own pool; in particular `ParseMedia` (default 1) shares nothing
/// with `Parse`, so a long recording never holds a document slot (D77).
pub fn slot_pools(
    cfg: &SchedulerConfig,
    kinds: impl IntoIterator<Item = JobKind>,
) -> HashMap<JobKind, Arc<Semaphore>> {
    kinds
        .into_iter()
        .map(|k| (k, Arc::new(Semaphore::new(slots_for(cfg, k)))))
        .collect()
}

async fn claim_loop(
    pool: PgPool,
    kind: JobKind,
    handler: Arc<dyn JobHandler>,
    cfg: SchedulerConfig,
    slots: Arc<Semaphore>,
    mut stop: watch::Receiver<bool>,
    wake: Arc<Notify>,
) {
    let mut backoff = Duration::from_secs(1);
    let mut running: Vec<JoinHandle<()>> = Vec::new();
    loop {
        if *stop.borrow() {
            break;
        }
        let permit = tokio::select! {
            p = slots.clone().acquire_owned() => match p {
                Ok(p) => p,
                Err(_) => break,
            },
            _ = stop.changed() => break,
        };
        match claim_one(&pool, kind, &cfg.owner).await {
            Ok(Some(job)) => {
                backoff = Duration::from_secs(1);
                let (pool, owner, handler) = (pool.clone(), cfg.owner.clone(), handler.clone());
                let heartbeat = cfg.heartbeat;
                running.retain(|h| !h.is_finished());
                running.push(tokio::spawn(async move {
                    run_job(pool, job, owner, handler, heartbeat).await;
                    drop(permit);
                }));
            }
            Ok(None) => {
                drop(permit);
                tokio::select! {
                    _ = tokio::time::sleep(cfg.poll_interval) => {}
                    _ = wake.notified() => {}
                    _ = stop.changed() => {}
                }
            }
            Err(_) => {
                // Database unavailable: stop claiming and back off 1 s -> 30 s (4.12).
                drop(permit);
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = stop.changed() => {}
                }
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
    for h in running {
        let _ = h.await;
    }
}

async fn claim_one(
    pool: &PgPool,
    kind: JobKind,
    owner: &str,
) -> Result<Option<Job>, crate::store::StoreError> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(crate::store::StoreError::from)?;
    let mut jobs = store::claim(&mut conn, &[kind.as_str()], owner, 1).await?;
    Ok(jobs.pop())
}

async fn run_job(
    pool: PgPool,
    job: Job,
    owner: String,
    handler: Arc<dyn JobHandler>,
    heartbeat: Duration,
) {
    let started = std::time::Instant::now();
    log_job("job.claimed", "debug", &job, started, None);
    let cancel = Arc::new(AtomicBool::new(false));
    let lost = Arc::new(AtomicBool::new(false));
    let hb = tokio::spawn(heartbeat_loop(
        pool.clone(),
        job.id.clone(),
        owner.clone(),
        job.generation,
        heartbeat,
        cancel.clone(),
        lost.clone(),
    ));
    let ctx = JobCtx {
        job: job.clone(),
        owner: owner.clone(),
        pool: pool.clone(),
        cancel,
        lost: lost.clone(),
    };
    // A panic must not take the claimer with it: run the handler in its own
    // task and treat a join error as a transient internal failure.
    let task = tokio::spawn(async move { handler.run(ctx).await });
    let outcome = match task.await {
        Ok(o) => o,
        Err(_) => Outcome::failed(FailureClass::Transient, "DOCS_ENGINE_INTERNAL"),
    };
    hb.abort();
    if lost.load(Ordering::Relaxed) {
        return;
    }
    let Ok(mut conn) = pool.acquire().await else {
        return; // the lease expires and the job is reclaimed
    };
    let code = match &outcome {
        Outcome::Failed { code, .. } => Some(code.clone()),
        _ => None,
    };
    let status = match outcome {
        Outcome::Completed => {
            log_job("job.completed", "debug", &job, started, None);
            return;
        }
        Outcome::Done => store::complete(&mut conn, &job.id, &owner, job.generation)
            .await
            .map(|r| r.status),
        Outcome::Cancelled => store::ack_cancel(&mut conn, &job.id, &owner, job.generation)
            .await
            .map(|r| r.status),
        Outcome::Failed { class, code } => report_failure(&mut conn, &job, &owner, class, &code)
            .await
            .map(|r| r.status),
    };
    match (&status, code.as_deref()) {
        (Ok(JobStatus::Quarantined), c) => log_job("job.quarantined", "warn", &job, started, c),
        (Ok(JobStatus::RetryScheduled), c) => log_job("job.retry", "info", &job, started, c),
        (Ok(JobStatus::Exhausted), c) => log_job("job.failed", "warn", &job, started, c),
        (Ok(_), None) => log_job("job.completed", "debug", &job, started, None),
        _ => {}
    }
}

/// One structured job line (10 §10.1). Fixed fields only; no path or name.
fn log_job(event: &str, level: &str, job: &Job, started: std::time::Instant, code: Option<&str>) {
    if level == "debug" && !crate::obs::debug_enabled() {
        return;
    }
    let f = crate::obs::Fields {
        job_id: Some(&job.id),
        kind: Some(&job.kind),
        drive_id: job.drive_id.as_deref(),
        file_id: job.file_id.as_deref(),
        attempt: Some(job.attempt),
        duration_ms: Some(i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)),
        code,
        ..crate::obs::Fields::default()
    };
    crate::obs::emit(level, event, &f);
}

async fn heartbeat_loop(
    pool: PgPool,
    id: String,
    owner: String,
    generation: i64,
    period: Duration,
    cancel: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
) {
    loop {
        tokio::time::sleep(period).await;
        let Ok(mut conn) = pool.acquire().await else {
            continue;
        };
        match store::heartbeat(&mut conn, &id, &owner, generation).await {
            Ok(r) => match r.status {
                JobStatus::Extended => {
                    if r.job.is_some_and(|j| j.cancel_requested) {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
                JobStatus::LostLease | JobStatus::Terminal => {
                    lost.store(true, Ordering::Relaxed);
                    return;
                }
                _ => {}
            },
            Err(_) => continue,
        }
    }
}
