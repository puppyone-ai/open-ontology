//! Job framework, state machine and event outbox against one throwaway
//! PostgreSQL (B-S2-04). Everything runs as a login member of
//! `oo_docs_runtime`; the superuser connection only plants fixtures and ages
//! leases.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test jobs_live -- --ignored`

mod common;

use std::collections::HashSet;

use common::*;
use oo_docs::jobs::{FailureClass, JobKind, JobStatus, NewJob, report_failure};
use oo_docs::state::{FileState, Transition, TransitionError, transition, transition_in_tx};
use oo_docs::store::{events, jobs as store};
use sqlx::postgres::PgListener;
use sqlx::{Executor, PgConnection};

const DRIVE: &str = "00000000-0000-0000-0000-0000000000d1";

async fn db(name: &str) -> PgConnection {
    installed_db(name).await;
    connect_runtime(name).await
}

async fn age_lease(admin: &mut PgConnection, id: &str) {
    sqlx::query(
        "UPDATE _oo_docs.job SET lease_until = clock_timestamp() - interval '1 second' WHERE id = $1::uuid",
    )
    .bind(id)
    .execute(admin)
    .await
    .unwrap();
}

async fn make_due(admin: &mut PgConnection, id: &str) {
    sqlx::query("UPDATE _oo_docs.job SET run_at = clock_timestamp() - interval '1 second' WHERE id = $1::uuid")
        .bind(id)
        .execute(admin)
        .await
        .unwrap();
}

async fn enqueue(rt: &mut PgConnection, kind: JobKind, key: &str, max: Option<i32>) -> String {
    let mut job = NewJob::new(kind, key);
    job.max_attempts = max;
    let r = store::enqueue(rt, &job).await.unwrap();
    assert_eq!(r.status, JobStatus::Enqueued, "{key}");
    r.job.unwrap().id
}

async fn claim_one(rt: &mut PgConnection, kind: JobKind, owner: &str) -> oo_docs::jobs::Job {
    let mut got = store::claim(rt, &[kind.as_str()], owner, 1).await.unwrap();
    assert_eq!(got.len(), 1, "nothing to claim for {owner}");
    got.remove(0)
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn lease_expiry_reclaim() {
    let _g = CLUSTER_LOCK.lock().await;
    let mut rt = db("oo_docs_jl_lease").await;
    let mut admin = connect("oo_docs_jl_lease").await;

    let id = enqueue(&mut rt, JobKind::Hash, "hash:a", Some(3)).await;
    let a = claim_one(&mut rt, JobKind::Hash, "host/1/aaaa").await;
    assert_eq!((a.attempt, a.generation), (1, 1));
    assert_eq!(a.lease_owner.as_deref(), Some("host/1/aaaa"));
    // A live lease is not stolen.
    assert!(
        store::claim(&mut rt, &["hash"], "host/2/bbbb", 5)
            .await
            .unwrap()
            .is_empty()
    );

    age_lease(&mut admin, &id).await;
    let b = claim_one(&mut rt, JobKind::Hash, "host/2/bbbb").await;
    assert_eq!(b.id, id);
    assert_eq!((b.attempt, b.generation), (2, 2));
    assert_eq!(b.lease_owner.as_deref(), Some("host/2/bbbb"));

    // The old worker lost the lease on every call.
    let hb = store::heartbeat(&mut rt, &id, "host/1/aaaa", 1)
        .await
        .unwrap();
    assert_eq!(hb.status, JobStatus::LostLease);
    let done = store::complete(&mut rt, &id, "host/1/aaaa", 1)
        .await
        .unwrap();
    assert_eq!(done.status, JobStatus::LostLease);
    let ok = store::complete(&mut rt, &id, "host/2/bbbb", 2)
        .await
        .unwrap();
    assert_eq!(ok.status, JobStatus::Succeeded);
    assert_eq!(
        store::complete(&mut rt, &id, "host/2/bbbb", 2)
            .await
            .unwrap()
            .status,
        JobStatus::Terminal
    );

    // Same worker id after a restart still gets a new generation.
    let id2 = enqueue(&mut rt, JobKind::Hash, "hash:b", Some(3)).await;
    let first = claim_one(&mut rt, JobKind::Hash, "host/1/aaaa").await;
    age_lease(&mut admin, &id2).await;
    let again = claim_one(&mut rt, JobKind::Hash, "host/1/aaaa").await;
    assert_eq!(again.generation, first.generation + 1);
    assert_eq!(
        store::complete(&mut rt, &id2, "host/1/aaaa", first.generation)
            .await
            .unwrap()
            .status,
        JobStatus::LostLease
    );

    // At the attempt cap an expired lease fails the job instead of requeueing.
    let id3 = enqueue(&mut rt, JobKind::Hash, "hash:c", Some(1)).await;
    claim_one(&mut rt, JobKind::Hash, "host/1/aaaa").await;
    age_lease(&mut admin, &id3).await;
    assert!(
        store::claim(&mut rt, &["hash"], "host/1/aaaa", 5)
            .await
            .unwrap()
            .is_empty()
    );
    let dead = store::get(&mut rt, &id3).await.unwrap().unwrap();
    assert_eq!(dead.state, "failed");
    assert_eq!(
        dead.last_error.as_deref(),
        Some("lease_expired_attempt_cap")
    );
    drop_db("oo_docs_jl_lease").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn heartbeat_rejects_stale_generation_and_expired_lease() {
    let _g = CLUSTER_LOCK.lock().await;
    let mut rt = db("oo_docs_jl_hb").await;
    let mut admin = connect("oo_docs_jl_hb").await;
    let id = enqueue(&mut rt, JobKind::Parse, "parse:x", None).await;
    let job = claim_one(&mut rt, JobKind::Parse, "host/1/aaaa").await;

    let until = |id: String| async {
        // lease_until in microseconds since epoch, read as admin.
        let mut a = connect("oo_docs_jl_hb").await;
        sqlx::query_scalar::<_, i64>(
            "SELECT (extract(epoch FROM lease_until) * 1000000)::bigint FROM _oo_docs.job WHERE id = $1::uuid",
        )
        .bind(id)
        .fetch_one(&mut a)
        .await
        .unwrap()
    };
    let before = until(id.clone()).await;

    for (owner, generation) in [
        ("host/1/aaaa", job.generation + 1),
        ("host/1/aaaa", job.generation + 100),
        ("host/9/zzzz", job.generation),
    ] {
        let r = store::heartbeat(&mut rt, &id, owner, generation)
            .await
            .unwrap();
        assert_eq!(r.status, JobStatus::LostLease, "{owner} {generation}");
    }
    assert_eq!(
        until(id.clone()).await,
        before,
        "stale heartbeat moved the lease"
    );

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let hb = store::heartbeat(&mut rt, &id, "host/1/aaaa", job.generation)
        .await
        .unwrap();
    assert_eq!(hb.status, JobStatus::Extended);
    assert!(!hb.job.unwrap().cancel_requested);
    assert!(until(id.clone()).await > before);

    // An expired lease is never revived by a late heartbeat.
    age_lease(&mut admin, &id).await;
    let late = store::heartbeat(&mut rt, &id, "host/1/aaaa", job.generation)
        .await
        .unwrap();
    assert_eq!(late.status, JobStatus::LostLease);
    // Unknown id.
    let ghost = store::heartbeat(
        &mut rt,
        "00000000-0000-0000-0000-00000000dead",
        "host/1/aaaa",
        1,
    )
    .await
    .unwrap();
    assert_eq!(ghost.status, JobStatus::LostLease);
    assert!(ghost.job.is_none());
    drop_db("oo_docs_jl_hb").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn cancel_semantics() {
    let _g = CLUSTER_LOCK.lock().await;
    let mut rt = db("oo_docs_jl_cancel").await;
    let mut admin = connect("oo_docs_jl_cancel").await;

    // queued -> cancelled, never claimed
    let q = enqueue(&mut rt, JobKind::Normalize, "norm:q", None).await;
    let r = store::cancel(&mut rt, &q).await.unwrap();
    assert_eq!(r.status, JobStatus::Cancelled);
    assert_eq!(r.job.unwrap().state, "cancelled");
    assert!(
        store::claim(&mut rt, &["normalize"], "host/1/aaaa", 5)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store::cancel(&mut rt, &q).await.unwrap().status,
        JobStatus::Terminal
    );
    assert_eq!(
        store::cancel(&mut rt, "00000000-0000-0000-0000-00000000dead")
            .await
            .unwrap()
            .status,
        JobStatus::NotFound
    );
    // a cancelled key is terminal for dedupe
    let again = store::enqueue(&mut rt, &NewJob::new(JobKind::Normalize, "norm:q"))
        .await
        .unwrap();
    assert_eq!(again.status, JobStatus::Terminal);

    // running -> flag; worker sees it on heartbeat and acknowledges
    let id = enqueue(&mut rt, JobKind::Normalize, "norm:r", None).await;
    let job = claim_one(&mut rt, JobKind::Normalize, "host/1/aaaa").await;
    assert_eq!(
        store::ack_cancel(&mut rt, &id, "host/1/aaaa", job.generation)
            .await
            .unwrap()
            .status,
        JobStatus::NotRequested
    );
    let flagged = store::cancel(&mut rt, &id).await.unwrap();
    assert_eq!(flagged.status, JobStatus::CancelRequested);
    assert_eq!(flagged.job.unwrap().state, "running");
    let hb = store::heartbeat(&mut rt, &id, "host/1/aaaa", job.generation)
        .await
        .unwrap();
    assert_eq!(hb.status, JobStatus::Extended);
    assert!(hb.job.unwrap().cancel_requested);
    let ack = store::ack_cancel(&mut rt, &id, "host/1/aaaa", job.generation)
        .await
        .unwrap();
    assert_eq!(ack.status, JobStatus::Cancelled);
    assert_eq!(ack.job.unwrap().state, "cancelled");

    // a worker that dies after the flag: the reclaim cancels instead of retrying
    let id2 = enqueue(&mut rt, JobKind::Normalize, "norm:s", None).await;
    claim_one(&mut rt, JobKind::Normalize, "host/1/aaaa").await;
    store::cancel(&mut rt, &id2).await.unwrap();
    age_lease(&mut admin, &id2).await;
    assert!(
        store::claim(&mut rt, &["normalize"], "host/2/bbbb", 5)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store::get(&mut rt, &id2).await.unwrap().unwrap().state,
        "cancelled"
    );

    // a running job may still finish if it ignores the flag; the work was done
    let id3 = enqueue(&mut rt, JobKind::Normalize, "norm:t", None).await;
    let j3 = claim_one(&mut rt, JobKind::Normalize, "host/1/aaaa").await;
    store::cancel(&mut rt, &id3).await.unwrap();
    assert_eq!(
        store::complete(&mut rt, &id3, "host/1/aaaa", j3.generation)
            .await
            .unwrap()
            .status,
        JobStatus::Succeeded
    );
    assert_eq!(
        store::cancel(&mut rt, &id3).await.unwrap().status,
        JobStatus::Terminal
    );
    drop_db("oo_docs_jl_cancel").await;
}

async fn delay_ms_from_now(admin: &mut PgConnection, id: &str) -> f64 {
    sqlx::query_scalar::<_, f64>(
        "SELECT (extract(epoch FROM run_at - clock_timestamp()) * 1000)::float8 FROM _oo_docs.job WHERE id = $1::uuid",
    )
    .bind(id)
    .fetch_one(admin)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn retry_with_capped_backoff() {
    let _g = CLUSTER_LOCK.lock().await;
    let mut rt = db("oo_docs_jl_retry").await;
    let mut admin = connect("oo_docs_jl_retry").await;

    // publish caps a caller delay at 30 s
    let id = enqueue(&mut rt, JobKind::Publish, "publish:1", Some(5)).await;
    let job = claim_one(&mut rt, JobKind::Publish, "host/1/aaaa").await;
    let r = store::retry(
        &mut rt,
        &id,
        "host/1/aaaa",
        job.generation,
        "DB_DEGRADED",
        10_000_000,
        true,
        false,
    )
    .await
    .unwrap();
    assert_eq!(r.status, JobStatus::RetryScheduled);
    let j = r.job.unwrap();
    assert_eq!(
        (j.state.as_str(), j.attempt, j.last_error.as_deref()),
        ("queued", 1, Some("DB_DEGRADED"))
    );
    let d = delay_ms_from_now(&mut admin, &id).await;
    assert!((29_000.0..=30_100.0).contains(&d), "{d}");
    assert!(
        store::claim(&mut rt, &["publish"], "host/1/aaaa", 5)
            .await
            .unwrap()
            .is_empty()
    );
    // the stale generation cannot retry again
    assert_eq!(
        store::retry(
            &mut rt,
            &id,
            "host/1/aaaa",
            job.generation,
            "X",
            0,
            true,
            false
        )
        .await
        .unwrap()
        .status,
        JobStatus::LostLease
    );

    make_due(&mut admin, &id).await;
    let second = claim_one(&mut rt, JobKind::Publish, "host/1/aaaa").await;
    assert_eq!((second.attempt, second.generation), (2, 2));
    // a negative delay means "now"; not counting the attempt gives it back
    let r = store::retry(
        &mut rt,
        &id,
        "host/1/aaaa",
        2,
        "PARSER_BUSY",
        -5,
        false,
        false,
    )
    .await
    .unwrap();
    assert_eq!(r.job.unwrap().attempt, 1);
    assert!(delay_ms_from_now(&mut admin, &id).await < 50.0);
    let third = claim_one(&mut rt, JobKind::Publish, "host/1/aaaa").await;
    assert_eq!(third.attempt, 2);

    // report_failure: transient parse failure waits about 30 s +-20%
    let pid = enqueue(&mut rt, JobKind::Parse, "parse:r", None).await;
    let pj = claim_one(&mut rt, JobKind::Parse, "host/1/aaaa").await;
    let r = report_failure(
        &mut rt,
        &pj,
        "host/1/aaaa",
        FailureClass::Transient,
        "DOCS_PARSE_TIMEOUT",
    )
    .await
    .unwrap();
    assert_eq!(r.status, JobStatus::RetryScheduled);
    let d = delay_ms_from_now(&mut admin, &pid).await;
    assert!((23_500.0..=36_100.0).contains(&d), "{d}");

    // plain retries run out at max_attempts and the job fails
    let eid = enqueue(&mut rt, JobKind::Hash, "hash:e", Some(2)).await;
    for n in 1..=2 {
        let c = claim_one(&mut rt, JobKind::Hash, "host/1/aaaa").await;
        assert_eq!(c.attempt, n);
        let r = store::retry(
            &mut rt,
            &eid,
            "host/1/aaaa",
            c.generation,
            "SOURCE_5XX",
            0,
            true,
            false,
        )
        .await
        .unwrap();
        if n == 1 {
            assert_eq!(r.status, JobStatus::RetryScheduled);
        } else {
            assert_eq!(r.status, JobStatus::Exhausted);
            assert_eq!(r.job.unwrap().state, "failed");
        }
    }
    // permanent failure
    let fid = enqueue(&mut rt, JobKind::Hash, "hash:f", None).await;
    let f = claim_one(&mut rt, JobKind::Hash, "host/1/aaaa").await;
    let r = report_failure(
        &mut rt,
        &f,
        "host/1/aaaa",
        FailureClass::Permanent,
        "DOCS_SOURCE_GONE",
    )
    .await
    .unwrap();
    assert_eq!(r.status, JobStatus::Failed);
    assert_eq!(
        store::get(&mut rt, &fid)
            .await
            .unwrap()
            .unwrap()
            .last_error
            .as_deref(),
        Some("DOCS_SOURCE_GONE")
    );
    // input faults are rejected by the function
    let bad = store::retry(&mut rt, &fid, "bad owner!", 1, "X", 0, true, false)
        .await
        .unwrap_err();
    assert_eq!(bad.sqlstate(), Some("OO010"));
    drop_db("oo_docs_jl_retry").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn quarantine_after_repeated_crashes() {
    let _g = CLUSTER_LOCK.lock().await;
    let mut rt = db("oo_docs_jl_poison").await;
    let mut admin = connect("oo_docs_jl_poison").await;

    let id = enqueue(&mut rt, JobKind::Parse, "parse:poison", None).await;
    let a = claim_one(&mut rt, JobKind::Parse, "host/1/aaaa").await;
    let r = report_failure(
        &mut rt,
        &a,
        "host/1/aaaa",
        FailureClass::Crash,
        "DOCS_PARSE_CRASH",
    )
    .await
    .unwrap();
    assert_eq!(r.status, JobStatus::RetryScheduled);
    assert_eq!(r.job.unwrap().crash_count, 1);
    make_due(&mut admin, &id).await;
    let b = claim_one(&mut rt, JobKind::Parse, "host/1/aaaa").await;
    let r = report_failure(
        &mut rt,
        &b,
        "host/1/aaaa",
        FailureClass::Crash,
        "DOCS_PARSE_CRASH",
    )
    .await
    .unwrap();
    assert_eq!(r.status, JobStatus::Quarantined);
    let q = r.job.unwrap();
    assert_eq!((q.state.as_str(), q.crash_count), ("quarantined", 2));
    // never handed out again, and the key stays terminal
    assert!(
        store::claim(&mut rt, &["parse"], "host/1/aaaa", 5)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store::enqueue(&mut rt, &NewJob::new(JobKind::Parse, "parse:poison"))
            .await
            .unwrap()
            .status,
        JobStatus::Terminal
    );

    // a crash at the attempt cap quarantines even on the first crash
    let id2 = enqueue(&mut rt, JobKind::Parse, "parse:last", Some(1)).await;
    let c = claim_one(&mut rt, JobKind::Parse, "host/1/aaaa").await;
    let r = report_failure(
        &mut rt,
        &c,
        "host/1/aaaa",
        FailureClass::Crash,
        "DOCS_PARSE_CRASH",
    )
    .await
    .unwrap();
    assert_eq!(r.status, JobStatus::Quarantined);
    assert_eq!(
        store::get(&mut rt, &id2)
            .await
            .unwrap()
            .unwrap()
            .crash_count,
        1
    );

    // busy errors neither count an attempt nor a crash
    let id3 = enqueue(&mut rt, JobKind::Parse, "parse:busy", Some(1)).await;
    for _ in 0..3 {
        let c = claim_one(&mut rt, JobKind::Parse, "host/1/aaaa").await;
        let r = report_failure(
            &mut rt,
            &c,
            "host/1/aaaa",
            FailureClass::Busy,
            "PARSER_BUSY",
        )
        .await
        .unwrap();
        assert_eq!(r.status, JobStatus::RetryScheduled);
        make_due(&mut admin, &id3).await;
    }
    let j = store::get(&mut rt, &id3).await.unwrap().unwrap();
    assert_eq!((j.attempt, j.crash_count), (0, 0));

    store::cancel(&mut rt, &id3).await.unwrap();

    // explicit quarantine by a worker
    let id4 = enqueue(&mut rt, JobKind::Parse, "parse:manual", None).await;
    let m = claim_one(&mut rt, JobKind::Parse, "host/1/aaaa").await;
    let r = store::quarantine(
        &mut rt,
        &id4,
        "host/1/aaaa",
        m.generation,
        "DOCS_PARSE_QUARANTINED",
    )
    .await
    .unwrap();
    assert_eq!(r.status, JobStatus::Quarantined);
    drop_db("oo_docs_jl_poison").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn dedupe_replay_and_already_done() {
    let _g = CLUSTER_LOCK.lock().await;
    let mut rt = db("oo_docs_jl_dedupe").await;
    let first = store::enqueue(&mut rt, &NewJob::new(JobKind::Hash, "hash:f1:abc"))
        .await
        .unwrap();
    assert_eq!(first.status, JobStatus::Enqueued);
    let id = first.job.unwrap().id;

    let replay = store::enqueue(&mut rt, &NewJob::new(JobKind::Hash, "hash:f1:abc"))
        .await
        .unwrap();
    assert_eq!(replay.status, JobStatus::Replayed);
    assert_eq!(replay.job.unwrap().id, id);
    // still a replay while running
    let job = claim_one(&mut rt, JobKind::Hash, "host/1/aaaa").await;
    assert_eq!(
        store::enqueue(&mut rt, &NewJob::new(JobKind::Hash, "hash:f1:abc"))
            .await
            .unwrap()
            .status,
        JobStatus::Replayed
    );
    store::complete(&mut rt, &id, "host/1/aaaa", job.generation)
        .await
        .unwrap();
    let done = store::enqueue(&mut rt, &NewJob::new(JobKind::Hash, "hash:f1:abc"))
        .await
        .unwrap();
    assert_eq!(done.status, JobStatus::AlreadyDone);
    assert_eq!(done.job.unwrap().id, id);

    // failed: terminal, the caller adds retry#n
    let f = enqueue(&mut rt, JobKind::Hash, "hash:f2", None).await;
    let fj = claim_one(&mut rt, JobKind::Hash, "host/1/aaaa").await;
    store::fail(&mut rt, &f, "host/1/aaaa", fj.generation, "DOCS_X")
        .await
        .unwrap();
    assert_eq!(
        store::enqueue(&mut rt, &NewJob::new(JobKind::Hash, "hash:f2"))
            .await
            .unwrap()
            .status,
        JobStatus::Terminal
    );
    assert_eq!(
        store::enqueue(&mut rt, &NewJob::new(JobKind::Hash, "hash:f2:retry#1"))
            .await
            .unwrap()
            .status,
        JobStatus::Enqueued
    );

    // invalid input never writes
    let e = store::enqueue(&mut rt, &NewJob::new(JobKind::Hash, ""))
        .await
        .unwrap_err();
    assert_eq!(e.sqlstate(), Some("OO010"));
    let long = "k".repeat(513);
    let e = store::enqueue(&mut rt, &NewJob::new(JobKind::Hash, &long))
        .await
        .unwrap_err();
    assert_eq!(e.sqlstate(), Some("OO010"));
    let mut orphan = NewJob::new(JobKind::Hash, "hash:orphan");
    orphan.file_id = Some("00000000-0000-0000-0000-0000000000ff");
    assert_eq!(
        store::enqueue(&mut rt, &orphan)
            .await
            .unwrap_err()
            .sqlstate(),
        Some("OO010")
    );
    drop_db("oo_docs_jl_dedupe").await;

    // concurrent enqueue of one key: exactly one wins
    installed_db("oo_docs_jl_dedupe2").await;
    let pool = pool_runtime("oo_docs_jl_dedupe2", 8).await;
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let pool = pool.clone();
        tasks.push(tokio::spawn(async move {
            let mut c = pool.acquire().await.unwrap();
            store::enqueue(&mut c, &NewJob::new(JobKind::Hash, "hash:race"))
                .await
                .unwrap()
                .status
        }));
    }
    let mut enqueued = 0;
    for t in tasks {
        match t.await.unwrap() {
            JobStatus::Enqueued => enqueued += 1,
            JobStatus::Replayed => {}
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(enqueued, 1);
    pool.close().await;
    drop_db("oo_docs_jl_dedupe2").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn claim_order_limit_kinds_and_checkpoint() {
    let _g = CLUSTER_LOCK.lock().await;
    let mut rt = db("oo_docs_jl_claim").await;
    let mut admin = connect("oo_docs_jl_claim").await;

    let mut low = NewJob::new(JobKind::Hash, "hash:low");
    low.priority = 900;
    let mut urgent = NewJob::new(JobKind::Hash, "hash:urgent");
    urgent.priority = 50;
    let normal = NewJob::new(JobKind::Hash, "hash:normal");
    let mut future = NewJob::new(JobKind::Hash, "hash:future");
    future.run_after_ms = 3_600_000;
    for j in [&low, &normal, &urgent, &future] {
        store::enqueue(&mut rt, j).await.unwrap();
    }
    let other = enqueue(&mut rt, JobKind::Parse, "parse:other", None).await;

    let got = store::claim(&mut rt, &["hash"], "host/1/aaaa", 2)
        .await
        .unwrap();
    let keys: Vec<_> = got.iter().map(|j| j.dedupe_key.as_str()).collect();
    assert_eq!(keys, ["hash:urgent", "hash:normal"]);
    let rest = store::claim(&mut rt, &["hash"], "host/1/aaaa", 10)
        .await
        .unwrap();
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].dedupe_key, "hash:low");
    assert!(
        store::claim(&mut rt, &["hash"], "host/1/aaaa", 10)
            .await
            .unwrap()
            .is_empty()
    );
    let p = store::claim(&mut rt, &["parse", "sync"], "host/1/aaaa", 10)
        .await
        .unwrap();
    assert_eq!(p[0].id, other);

    // checkpoint survives a lease expiry and is guarded by the lease
    let g = p[0].generation;
    let saved = store::save_checkpoint(
        &mut rt,
        &other,
        "host/1/aaaa",
        g,
        r#"{"windows_done":[[0,8]]}"#,
    )
    .await
    .unwrap();
    assert_eq!(saved.status, JobStatus::Saved);
    assert_eq!(
        store::save_checkpoint(&mut rt, &other, "host/1/aaaa", g + 1, "{}")
            .await
            .unwrap()
            .status,
        JobStatus::LostLease
    );
    age_lease(&mut admin, &other).await;
    let again = claim_one(&mut rt, JobKind::Parse, "host/2/bbbb").await;
    let cp: serde_json::Value = serde_json::from_str(again.checkpoint.as_deref().unwrap()).unwrap();
    assert_eq!(cp["windows_done"][0][1], 8);

    // bad claim arguments
    assert_eq!(
        store::claim(&mut rt, &["hash"], "bad owner", 1)
            .await
            .unwrap_err()
            .sqlstate(),
        Some("OO010")
    );
    assert_eq!(
        store::claim(&mut rt, &["hash"], "host/1/aaaa", 0)
            .await
            .unwrap_err()
            .sqlstate(),
        Some("OO010")
    );
    drop_db("oo_docs_jl_claim").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn concurrent_claimers_never_share_a_job() {
    let _g = CLUSTER_LOCK.lock().await;
    installed_db("oo_docs_jl_skip").await;
    let pool = pool_runtime("oo_docs_jl_skip", 8).await;
    {
        let mut c = pool.acquire().await.unwrap();
        for n in 0..120 {
            let key = format!("hash:{n}");
            store::enqueue(&mut c, &NewJob::new(JobKind::Hash, &key))
                .await
                .unwrap();
        }
    }
    let mut tasks = Vec::new();
    for w in 0..8 {
        let pool = pool.clone();
        tasks.push(tokio::spawn(async move {
            let owner = format!("host/{w}/aaaa");
            let mut mine = Vec::new();
            loop {
                let mut c = pool.acquire().await.unwrap();
                let got = store::claim(&mut c, &["hash"], &owner, 7).await.unwrap();
                if got.is_empty() {
                    return mine;
                }
                mine.extend(got.into_iter().map(|j| j.id));
            }
        }));
    }
    let mut all = Vec::new();
    for t in tasks {
        all.extend(t.await.unwrap());
    }
    let unique: HashSet<_> = all.iter().collect();
    assert_eq!(all.len(), 120);
    assert_eq!(unique.len(), 120, "a job was leased twice");
    pool.close().await;
    drop_db("oo_docs_jl_skip").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn event_seq_is_gap_free_under_concurrent_appenders() {
    let _g = CLUSTER_LOCK.lock().await;
    installed_db("oo_docs_jl_events").await;
    let pool = pool_runtime("oo_docs_jl_events", 8).await;
    const WRITERS: usize = 8;
    const EACH: usize = 200;

    let mut listener = PgListener::connect_with(&pool).await.unwrap();
    listener.listen(events::NOTIFY_CHANNEL).await.unwrap();

    let mut tasks = Vec::new();
    for w in 0..WRITERS {
        let pool = pool.clone();
        tasks.push(tokio::spawn(async move {
            let mut seqs = Vec::with_capacity(EACH);
            for n in 0..EACH {
                let mut tx = pool.begin().await.unwrap();
                let payload = format!("{{\"writer\":{w},\"n\":{n}}}");
                let seq = events::append_event(&mut tx, "test.appended", None, None, &payload)
                    .await
                    .unwrap();
                if n % 25 == 7 {
                    // a rolled back append must not leave a hole
                    tx.rollback().await.unwrap();
                    continue;
                }
                tx.commit().await.unwrap();
                seqs.push((seq, n));
            }
            seqs
        }));
    }
    let mut returned = Vec::new();
    for t in tasks {
        let seqs = t.await.unwrap();
        assert!(
            seqs.windows(2).all(|p| p[0].0 < p[1].0),
            "writer order broke"
        );
        returned.extend(seqs.into_iter().map(|(s, _)| s));
    }
    let committed = returned.len() as i64;
    assert_eq!(committed, (WRITERS * (EACH - 8)) as i64);

    let mut admin = connect("oo_docs_jl_events").await;
    let stored: Vec<i64> = sqlx::query_scalar("SELECT seq FROM _oo_docs.event ORDER BY seq")
        .fetch_all(&mut admin)
        .await
        .unwrap();
    assert_eq!(
        stored,
        (1..=committed).collect::<Vec<_>>(),
        "seq is not 1..N"
    );
    let mut sorted = returned.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, stored);
    let head: i64 = sqlx::query_scalar("SELECT seq FROM _oo_docs.event_head")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert_eq!(head, committed);

    // seq order is commit order (needs track_commit_timestamp=on in the container)
    let ts: Vec<i64> = sqlx::query_scalar(
        "SELECT (extract(epoch FROM pg_xact_commit_timestamp(xmin)) * 1000000)::bigint \
         FROM _oo_docs.event ORDER BY seq",
    )
    .fetch_all(&mut admin)
    .await
    .unwrap();
    assert_eq!(ts.len() as i64, committed);
    assert!(
        ts.windows(2).all(|p| p[0] <= p[1]),
        "seq order differs from commit order"
    );

    // a since cursor sees everything exactly once
    let mut rt = pool.acquire().await.unwrap();
    let mut cursor = 0;
    let mut seen = 0;
    loop {
        let page = events::read_since(&mut rt, cursor, 500).await.unwrap();
        if page.is_empty() {
            break;
        }
        for e in &page {
            assert_eq!(e.seq, seen as i64 + 1);
            seen += 1;
        }
        cursor = page.last().unwrap().seq;
    }
    assert_eq!(seen as i64, committed);
    assert_eq!(events::head(&mut rt).await.unwrap(), committed);

    // NOTIFY fired per commit (rollbacks send nothing)
    let mut notified = 0;
    while let Ok(Ok(Some(_))) =
        tokio::time::timeout(std::time::Duration::from_millis(200), listener.try_recv()).await
    {
        notified += 1;
    }
    assert_eq!(notified as i64, committed);

    // bad inputs
    assert_eq!(
        events::append_event(&mut rt, "Bad Type", None, None, "{}")
            .await
            .unwrap_err()
            .sqlstate(),
        Some("OO010")
    );
    assert_eq!(
        events::append_event(&mut rt, "a.b", None, None, "[]")
            .await
            .unwrap_err()
            .sqlstate(),
        Some("OO010")
    );
    assert_eq!(
        events::append_event(&mut rt, "a.b", None, None, r#"{"password":"x"}"#)
            .await
            .unwrap_err()
            .sqlstate(),
        Some("OO010")
    );
    drop(rt);
    drop(listener);
    pool.close().await;
    drop_db("oo_docs_jl_events").await;
}

async fn plant_file(admin: &mut PgConnection, id: &str, path: &str, state: &str) {
    sqlx::query(
        "INSERT INTO _oo_docs.file (id, drive_id, path, raw_key, path_fold, state, last_seen_generation) \
         VALUES ($1::uuid, $2::uuid, $3, convert_to($3, 'UTF8'), $3, $4, 1)",
    )
    .bind(id)
    .bind(DRIVE)
    .bind(path)
    .bind(state)
    .execute(admin)
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn transition_writes_state_history_and_event_together() {
    let _g = CLUSTER_LOCK.lock().await;
    installed_db("oo_docs_jl_state").await;
    let mut admin = connect("oo_docs_jl_state").await;
    admin
        .execute(sqlx::AssertSqlSafe(format!(
            "INSERT INTO _oo_docs.drive (id, name, backend, config) VALUES ('{DRIVE}', 'd', 'fs', '{{}}')"
        )))
        .await
        .unwrap();
    let f1 = "00000000-0000-0000-0000-0000000000f1";
    let f2 = "00000000-0000-0000-0000-0000000000f2";
    plant_file(&mut admin, f1, "a.md", "stored").await;
    plant_file(&mut admin, f2, "b.md", "queued").await;
    let pool = pool_runtime("oo_docs_jl_state", 8).await;

    let t = |file_id: &'static str, to| Transition {
        file_id,
        expect_from: None,
        to,
        code: None,
        retryable: false,
        version_id: None,
        attempt: None,
        detail_json: None,
    };
    let done = transition(&pool, &t(f1, FileState::Queued)).await.unwrap();
    assert_eq!(
        (done.from, done.to, done.history_seq, done.event_seq),
        (FileState::Stored, FileState::Queued, 1, 1)
    );
    let (state, rev): (String, i64) =
        sqlx::query_as("SELECT state, revision FROM _oo_docs.file WHERE id = $1::uuid")
            .bind(f1)
            .fetch_one(&mut admin)
            .await
            .unwrap();
    assert_eq!((state.as_str(), rev), ("queued", 2));
    let (from, to, subject, payload): (String, String, String, String) = sqlx::query_as(
        "SELECT h.from_state, h.to_state, e.subject, e.payload::text FROM _oo_docs.file_state_history h, _oo_docs.event e \
         WHERE h.file_id = $1::uuid AND e.seq = 1",
    )
    .bind(f1)
    .fetch_one(&mut admin)
    .await
    .unwrap();
    assert_eq!(
        (from.as_str(), to.as_str(), subject.as_str()),
        ("stored", "queued", f1)
    );
    assert!(payload.contains("\"to\": \"queued\""), "{payload}");

    // illegal, conflicting and unknown writes leave no trace
    let head = events::head(&mut admin).await.unwrap();
    let e = transition(&pool, &t(f1, FileState::Published))
        .await
        .unwrap_err();
    assert!(
        matches!(
            e,
            TransitionError::Illegal {
                from: FileState::Queued,
                to: FileState::Published
            }
        ),
        "{e}"
    );
    let mut c = t(f1, FileState::Parsing);
    c.expect_from = Some(FileState::Stored);
    assert!(matches!(
        transition(&pool, &c).await,
        Err(TransitionError::Conflict { .. })
    ));
    assert!(matches!(
        transition(
            &pool,
            &t("00000000-0000-0000-0000-00000000dead", FileState::Queued)
        )
        .await,
        Err(TransitionError::NotFound)
    ));
    assert_eq!(events::head(&mut admin).await.unwrap(), head);
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.file_state_history WHERE file_id = $1::uuid",
    )
    .bind(f1)
    .fetch_one(&mut admin)
    .await
    .unwrap();
    assert_eq!(n, 1);

    // inside a caller transaction a rollback undoes state, history and event
    let mut tx = pool.begin().await.unwrap();
    transition_in_tx(&mut tx, &t(f1, FileState::Parsing))
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM _oo_docs.file WHERE id = $1::uuid")
        .bind(f1)
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert_eq!(state, "queued");
    assert_eq!(events::head(&mut admin).await.unwrap(), head);

    // a stage finishes its job and moves the file in one commit
    let job_id = enqueue(
        &mut pool.acquire().await.unwrap(),
        JobKind::Parse,
        "parse:f1",
        None,
    )
    .await;
    let job = claim_one(
        &mut pool.acquire().await.unwrap(),
        JobKind::Parse,
        "host/1/aaaa",
    )
    .await;
    let mut tx = pool.begin().await.unwrap();
    let mut req = t(f1, FileState::Parsing);
    req.attempt = Some(job.attempt);
    req.detail_json = Some(r#"{"job":"parse"}"#);
    transition_in_tx(&mut tx, &req).await.unwrap();
    assert_eq!(
        store::complete(&mut tx, &job_id, "host/1/aaaa", job.generation)
            .await
            .unwrap()
            .status,
        JobStatus::Succeeded
    );
    tx.commit().await.unwrap();
    let (a, d): (Option<i32>, Option<String>) = sqlx::query_as(
        "SELECT attempt, detail::text FROM _oo_docs.file_state_history WHERE file_id = $1::uuid AND seq = 2",
    )
    .bind(f1).fetch_one(&mut admin).await.unwrap();
    assert_eq!(a, Some(1));
    assert!(d.unwrap().contains("parse"));

    // concurrent movers of one file: exactly one wins, history stays contiguous
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let pool = pool.clone();
        tasks.push(tokio::spawn(async move {
            let mut r = Transition {
                file_id: f2,
                expect_from: Some(FileState::Queued),
                to: FileState::Parsing,
                code: None,
                retryable: false,
                version_id: None,
                attempt: None,
                detail_json: None,
            };
            r.code = Some("DOCS_TEST");
            transition(&pool, &r).await.is_ok()
        }));
    }
    let mut wins = 0;
    for t in tasks {
        wins += usize::from(t.await.unwrap());
    }
    assert_eq!(wins, 1);
    let seqs: Vec<i64> = sqlx::query_scalar(
        "SELECT seq FROM _oo_docs.file_state_history WHERE file_id = $1::uuid ORDER BY seq",
    )
    .bind(f2)
    .fetch_all(&mut admin)
    .await
    .unwrap();
    assert_eq!(seqs, vec![1]);

    // history is insert-only for the runtime too
    let mut rt = connect_runtime("oo_docs_jl_state").await;
    assert_eq!(
        sqlstate(
            rt.execute("UPDATE _oo_docs.file_state_history SET code = 'x'")
                .await
        ),
        "42501"
    );
    pool.close().await;
    drop_db("oo_docs_jl_state").await;
}

/// 10 4.1.6: a parse job that reaches its attempt cap is quarantined, whether
/// the last counted failure was a crash, a timeout or an expired lease; other
/// kinds fail at the cap, and an explicit permanent failure stays `failed`.
#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn the_parse_attempt_cap_ends_in_quarantine() {
    let name = "oo_docs_jl_cap_quarantine";
    let _g = CLUSTER_LOCK.lock().await;
    installed_db(name).await;
    let mut rt = connect_runtime(name).await;
    let mut admin = connect(name).await;
    let owner = "host/1/aaaa";

    // A counted, non crash failure (a timeout) at the cap.
    let id = enqueue(&mut rt, JobKind::Parse, "parse:cap-timeout", Some(2)).await;
    let first = claim_one(&mut rt, JobKind::Parse, owner).await;
    let r = report_failure(
        &mut rt,
        &first,
        owner,
        FailureClass::Transient,
        "DOCS_PARSE_TIMEOUT",
    )
    .await
    .unwrap();
    assert_eq!(r.status, JobStatus::RetryScheduled);
    make_due(&mut admin, &id).await;
    let second = claim_one(&mut rt, JobKind::Parse, owner).await;
    assert_eq!(second.attempt, 2);
    let r = report_failure(
        &mut rt,
        &second,
        owner,
        FailureClass::Transient,
        "DOCS_PARSE_TIMEOUT",
    )
    .await
    .unwrap();
    assert_eq!(r.status, JobStatus::Quarantined, "attempt cap, no crash");
    let dead = store::get(&mut rt, &id).await.unwrap().unwrap();
    assert_eq!((dead.state.as_str(), dead.crash_count), ("quarantined", 0));
    assert_eq!(dead.last_error.as_deref(), Some("DOCS_PARSE_TIMEOUT"));
    // Terminal and absorbing.
    assert_eq!(
        store::complete(&mut rt, &id, owner, second.generation)
            .await
            .unwrap()
            .status,
        JobStatus::Terminal
    );

    // A lease that ends at the cap.
    let id = enqueue(&mut rt, JobKind::Parse, "parse:cap-lease", Some(1)).await;
    claim_one(&mut rt, JobKind::Parse, owner).await;
    age_lease(&mut admin, &id).await;
    assert!(
        store::claim(&mut rt, &["parse"], owner, 5)
            .await
            .unwrap()
            .is_empty()
    );
    let dead = store::get(&mut rt, &id).await.unwrap().unwrap();
    assert_eq!(dead.state, "quarantined");
    assert_eq!(
        dead.last_error.as_deref(),
        Some("lease_expired_attempt_cap")
    );

    // Below the cap an expired lease still just requeues.
    let id = enqueue(&mut rt, JobKind::Parse, "parse:below", Some(3)).await;
    claim_one(&mut rt, JobKind::Parse, owner).await;
    age_lease(&mut admin, &id).await;
    let again = claim_one(&mut rt, JobKind::Parse, owner).await;
    assert_eq!((again.id.as_str(), again.attempt), (id.as_str(), 2));

    // Other kinds fail at the cap, both ways.
    let id = enqueue(&mut rt, JobKind::Hash, "hash:cap-timeout", Some(1)).await;
    let job = claim_one(&mut rt, JobKind::Hash, owner).await;
    let r = report_failure(
        &mut rt,
        &job,
        owner,
        FailureClass::Transient,
        "DOCS_SOURCE_TIMEOUT",
    )
    .await
    .unwrap();
    assert_eq!(r.status, JobStatus::Exhausted);
    assert_eq!(
        store::get(&mut rt, &id).await.unwrap().unwrap().state,
        "failed"
    );

    // An explicit permanent failure of a parse job is `failed`, not a poison pill.
    let id = enqueue(&mut rt, JobKind::Parse, "parse:permanent", Some(3)).await;
    let job = claim_one(&mut rt, JobKind::Parse, owner).await;
    let r = report_failure(
        &mut rt,
        &job,
        owner,
        FailureClass::Permanent,
        "DOCS_PARSE_ENCODING",
    )
    .await
    .unwrap();
    assert_eq!(r.status, JobStatus::Failed);
    assert_eq!(
        store::get(&mut rt, &id).await.unwrap().unwrap().state,
        "failed"
    );
    drop_db(name).await;
}
