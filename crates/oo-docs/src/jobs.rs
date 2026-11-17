//! Job framework: lease, heartbeat, retry, cancel (10 §4.1). Board WS-BE5.
//!
//! The SQL side (`_oo_docs.enqueue/claim/heartbeat/complete/fail/retry/
//! quarantine/cancel`) owns the lease state machine; this module holds the
//! types, the per-kind retry policy (4.1.4), and failure classification.
//! Formal: FM1 (lease safety) is implemented by the `(owner, generation)` CAS
//! in those functions; the Rust wrappers in `store::jobs` never bypass it.

pub mod backoff;

use crate::store::StoreError;
use crate::store::jobs as store;
use backoff::Backoff;
use sqlx::PgConnection;

/// Lease length. The SQL functions use the same constant.
pub const LEASE_SECONDS: u64 = 30;
/// Intended heartbeat period; not a second lease length.
pub const HEARTBEAT_SECONDS: u64 = 10;
/// A blob whose parse crashes a child this many times is quarantined (4.1.6).
pub const CRASH_QUARANTINE_LIMIT: i32 = 2;

/// `_oo_docs.job.kind` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JobKind {
    /// Source listing.
    Sync,
    /// Fingerprint and blob hash.
    Hash,
    /// Parser run.
    Parse,
    /// Doc-IR normalization.
    Normalize,
    /// Deterministic extraction.
    ExtractRules,
    /// LLM extraction.
    ExtractLlm,
    /// Entity resolution.
    Resolve,
    /// Publish a version.
    Publish,
    /// Compile a Context Pack.
    Compile,
    /// Garbage collection.
    Gc,
    /// Share expiry.
    Expire,
    /// Drift detection.
    Drift,
    /// Reprocess after a parser or prompt upgrade.
    Reprocess,
}

impl JobKind {
    /// Every kind, in the CHECK constraint order.
    pub const ALL: [JobKind; 13] = [
        Self::Sync,
        Self::Hash,
        Self::Parse,
        Self::Normalize,
        Self::ExtractRules,
        Self::ExtractLlm,
        Self::Resolve,
        Self::Publish,
        Self::Compile,
        Self::Gc,
        Self::Expire,
        Self::Drift,
        Self::Reprocess,
    ];

    /// Database text.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Hash => "hash",
            Self::Parse => "parse",
            Self::Normalize => "normalize",
            Self::ExtractRules => "extract_rules",
            Self::ExtractLlm => "extract_llm",
            Self::Resolve => "resolve",
            Self::Publish => "publish",
            Self::Compile => "compile",
            Self::Gc => "gc",
            Self::Expire => "expire",
            Self::Drift => "drift",
            Self::Reprocess => "reprocess",
        }
    }

    /// Parse database text.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == text)
    }

    /// Retry policy from the 4.1.4 table.
    pub fn policy(self) -> RetryPolicy {
        const MIN: u64 = 60_000;
        let (max_attempts, base_ms, factor, cap_ms) = match self {
            // Periodic tasks: effectively unlimited, bounded by the column cap.
            Self::Sync => (32, 5_000, 2, 5 * MIN),
            Self::Hash => (6, 2_000, 4, 10 * MIN),
            // 3 attempts, then manual (4.1.6).
            Self::Parse => (3, 30_000, 4, 30 * MIN),
            Self::Normalize | Self::ExtractRules => (3, 10_000, 4, 10 * MIN),
            Self::ExtractLlm => (5, 5_000, 3, 5 * MIN),
            Self::Publish => (32, 1_000, 2, 30_000),
            Self::Compile => (2, 0, 1, 30_000),
            Self::Resolve | Self::Gc | Self::Expire | Self::Drift | Self::Reprocess => {
                (3, 10_000, 4, 10 * MIN)
            }
        };
        RetryPolicy {
            max_attempts,
            backoff: Backoff {
                base_ms,
                factor,
                cap_ms,
            },
        }
    }
}

/// Attempt budget and delay schedule for one kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Value for `job.max_attempts` (1..=32).
    pub max_attempts: i32,
    /// Delay schedule.
    pub backoff: Backoff,
}

/// `_oo_docs.job.state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    /// Waiting for `run_at` and a worker.
    Queued,
    /// Leased.
    Running,
    /// Finished.
    Succeeded,
    /// Gave up.
    Failed,
    /// Cancelled.
    Cancelled,
    /// Poison pill, manual retry only.
    Quarantined,
}

impl JobState {
    /// Parse database text.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "quarantined" => Self::Quarantined,
            _ => return None,
        })
    }

    /// No further automatic transition.
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Queued | Self::Running)
    }
}

/// Outcome text returned by the SQL functions (`job_result.status`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobStatus {
    /// New row inserted.
    Enqueued,
    /// Same dedupe key, still queued or running.
    Replayed,
    /// Same dedupe key, already succeeded.
    AlreadyDone,
    /// Same dedupe key, failed, cancelled or quarantined.
    Terminal,
    /// Leased by `claim`.
    Claimed,
    /// Lease extended.
    Extended,
    /// `(owner, generation)` no longer holds the lease; discard local results.
    LostLease,
    /// Finished successfully.
    Succeeded,
    /// Finished failed.
    Failed,
    /// Back in the queue after a delay.
    RetryScheduled,
    /// Attempt cap reached on a plain retry; now failed.
    Exhausted,
    /// Poison pill.
    Quarantined,
    /// Queued job cancelled.
    Cancelled,
    /// Running job flagged; the worker stops at its next checkpoint.
    CancelRequested,
    /// `ack_cancel` without a prior `cancel`.
    NotRequested,
    /// Checkpoint stored.
    Saved,
    /// No such job.
    NotFound,
    /// Something this version does not know.
    Other(String),
}

impl JobStatus {
    /// Parse database text.
    pub fn parse(text: &str) -> Self {
        match text {
            "enqueued" => Self::Enqueued,
            "replayed" => Self::Replayed,
            "already_done" => Self::AlreadyDone,
            "terminal" => Self::Terminal,
            "claimed" => Self::Claimed,
            "extended" => Self::Extended,
            "lost_lease" => Self::LostLease,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "retry_scheduled" => Self::RetryScheduled,
            "exhausted" => Self::Exhausted,
            "quarantined" => Self::Quarantined,
            "cancelled" => Self::Cancelled,
            "cancel_requested" => Self::CancelRequested,
            "saved" => Self::Saved,
            "not_requested" => Self::NotRequested,
            "not_found" => Self::NotFound,
            other => Self::Other(other.to_string()),
        }
    }
}

/// Job row as returned by the functions. Ids are canonical UUID text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    /// `job.id`.
    pub id: String,
    /// `job.kind`.
    pub kind: String,
    /// `job.state`.
    pub state: String,
    /// Attempts taken so far.
    pub attempt: i32,
    /// Lease generation; the CAS token with the owner.
    pub generation: i64,
    /// Current lease owner.
    pub lease_owner: Option<String>,
    /// Crashes attributed to this job.
    pub crash_count: i32,
    /// Set by `cancel` while running; checked at every checkpoint.
    pub cancel_requested: bool,
    /// Last error code.
    pub last_error: Option<String>,
    /// Resume state as JSON text.
    pub checkpoint: Option<String>,
    /// Drive.
    pub drive_id: Option<String>,
    /// File.
    pub file_id: Option<String>,
    /// File version.
    pub version_id: Option<String>,
    /// Idempotency key.
    pub dedupe_key: String,
    /// Smaller runs first.
    pub priority: i16,
    /// Attempt cap.
    pub max_attempts: i32,
}

/// Result of one job function call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobResult {
    /// Outcome.
    pub status: JobStatus,
    /// Row after the call; `None` for `lost_lease` on a missing row.
    pub job: Option<Job>,
}

/// New job.
#[derive(Clone, Debug)]
pub struct NewJob<'a> {
    /// Kind.
    pub kind: JobKind,
    /// Idempotency key (4.1.2), at most 512 bytes.
    pub dedupe_key: &'a str,
    /// Drive.
    pub drive_id: Option<&'a str>,
    /// File.
    pub file_id: Option<&'a str>,
    /// Version.
    pub version_id: Option<&'a str>,
    /// Priority; new file 100, manual retry 50, outdated 900.
    pub priority: i16,
    /// `None` uses the kind policy.
    pub max_attempts: Option<i32>,
    /// Milliseconds from now until the job is due.
    pub run_after_ms: i64,
}

impl<'a> NewJob<'a> {
    /// Defaults: priority 100, due now, kind policy attempts.
    pub fn new(kind: JobKind, dedupe_key: &'a str) -> Self {
        Self {
            kind,
            dedupe_key,
            drive_id: None,
            file_id: None,
            version_id: None,
            priority: 100,
            max_attempts: None,
            run_after_ms: 0,
        }
    }
}

/// How a worker classifies a stage failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass {
    /// Counts an attempt; retried with backoff until the cap.
    Transient,
    /// Does not count (`PARSER_BUSY`, sidecar unavailable); short delay.
    Busy,
    /// The single request of a child process died; counts toward quarantine.
    Crash,
    /// Not worth retrying.
    Permanent,
}

/// Worker id `<hostname>/<pid>/<start nonce>` (4.1.1). `nonce` is supplied by
/// the caller (the engine draws it once at start).
pub fn worker_id(hostname: &str, pid: u32, nonce: u64) -> String {
    let host: String = hostname
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_.:-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    let host = if host.is_empty() {
        "host".to_string()
    } else {
        host
    };
    format!("{host}/{pid}/{nonce:016x}")
}

/// Report a failed attempt for a leased job. The delay comes from the kind's
/// backoff with deterministic jitter; the server clamps it again.
pub async fn report_failure(
    conn: &mut PgConnection,
    job: &Job,
    owner: &str,
    class: FailureClass,
    code: &str,
) -> Result<JobResult, StoreError> {
    let kind = JobKind::parse(&job.kind)
        .ok_or_else(|| StoreError::Protocol(format!("unknown job kind {}", job.kind)))?;
    let backoff = kind.policy().backoff;
    match class {
        FailureClass::Permanent => store::fail(conn, &job.id, owner, job.generation, code).await,
        FailureClass::Transient => {
            let delay = backoff.delay_ms(&job.id, job.attempt.max(1) as u32);
            store::retry(
                conn,
                &job.id,
                owner,
                job.generation,
                code,
                delay,
                true,
                false,
            )
            .await
        }
        FailureClass::Busy => {
            let delay = backoff.base_ms.min(backoff.cap_ms) as i64;
            store::retry(
                conn,
                &job.id,
                owner,
                job.generation,
                code,
                delay,
                false,
                false,
            )
            .await
        }
        FailureClass::Crash => {
            let delay = backoff.delay_ms(&job.id, job.attempt.max(1) as u32);
            store::retry(
                conn,
                &job.id,
                owner,
                job.generation,
                code,
                delay,
                true,
                true,
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip_and_policies_fit_the_column() {
        for kind in JobKind::ALL {
            assert_eq!(JobKind::parse(kind.as_str()), Some(kind));
            let p = kind.policy();
            assert!((1..=32).contains(&p.max_attempts), "{kind:?}");
            assert!(p.backoff.base_ms <= p.backoff.cap_ms, "{kind:?}");
        }
        assert_eq!(JobKind::parse("nope"), None);
    }

    #[test]
    fn parse_policy_is_three_attempts() {
        assert_eq!(JobKind::Parse.policy().max_attempts, 3);
        assert_eq!(JobKind::Hash.policy().max_attempts, 6);
    }

    #[test]
    fn worker_id_is_accepted_by_the_sql_pattern() {
        let id = worker_id("my host", 42, 0xabc);
        assert_eq!(id, "my_host/42/0000000000000abc");
        assert!(id.len() <= 128);
    }

    #[test]
    fn states_and_status_parse() {
        assert!(JobState::parse("quarantined").unwrap().is_terminal());
        assert!(!JobState::parse("running").unwrap().is_terminal());
        assert_eq!(JobStatus::parse("lost_lease"), JobStatus::LostLease);
        assert_eq!(JobStatus::parse("x"), JobStatus::Other("x".into()));
    }
}
