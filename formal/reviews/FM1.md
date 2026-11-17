# FM1 review: job lease protocol (`formal/quint/jobs.qnt`)

Model of the implemented protocol: `crates/oo-pipeline/migrations/0138_docs_sources_jobs.sql`
(job table and the `_oo_docs.*` job functions), Rust wrappers `crates/oo-docs/src/{jobs.rs,store/jobs.rs}`.
Spec: 10 section 4.1, 80 sections 3.1 (FM1), 4, 5. Card V-S1-02, conformance V-S2-01.

Review status: author mapping and checks done. The 80 section 5 step 2 independent review
(implementer lead plus a native sub-agent) is **not yet done**; this document is the input for it.

## 1. Model shape

- One job kind; job index = dedupe key (`UNIQUE (dedupe_key)` means one row per key).
- State per job: `st` (absent, queued, running, succeeded, failed, cancelled, quarantined), `attempt`,
  `gen` (generation), `owner` (worker or -1), `live` (`lease_until > now`), `crash` (`crash_count`),
  `cancel` (`cancel_requested`). History variables: `successes` (times `complete` returned
  `succeeded`), `rows`, `term` (first terminal state), `prevGen`, last op and result.
- Time is not a number. `live` flips to false only through the `expire` action (time passing; this is
  also how a worker crash looks: the worker stops acting and its lease runs out).
- Bounds (default): 3 workers, 4 jobs, `max_attempts` 3 (the parse kind), crash limit 2, claims stop
  at generation 6 (the `g` argument of stale operations ranges over 1..6).
- Every lease operation takes an arbitrary `(worker, job, generation)`, so wrong owner, stale
  generation, unknown job, and terminal job are all explored.

## 2. 10 section 4.1 rules to model actions and checks

| Spec rule | SQL / Rust | Model | Checked by |
|---|---|---|---|
| 4.1.1 `claim` leases one due job, `FOR UPDATE SKIP LOCKED` | `claim()` second loop | `claim(w,j)`, `claimNone(w)` | `invLeaseShape`, `invSingleLiveLease`, witnesses |
| lease 30 s | `interval '30 seconds'` | `live` set true by claim and heartbeat; false by `expire` | not a number; only the live/expired distinction is modelled |
| heartbeat extends only a live matching lease, returns `cancel_requested` | `heartbeat()` | `leaseStep("heartbeat")` result `extended`; `cancel` flag stays visible in state | `invStaleRejected`, conformance |
| expired lease goes back to the queue at the next claim, `attempt` and `generation` rise with the next lease | `claim()` first loop (`queued`, or `failed` at the attempt cap, or `cancelled` when `cancel_requested`) | `reclaimOne`, applied to every expired job inside `claim`/`claimNone` (job count 4 is below the 8 row cap per call) | `invNoLostJob`, `witReclaimed`, `witExhausted` |
| all finish-type calls CAS on `(owner, generation)`; a worker that lost the lease is rejected (`lost_lease`) and must drop its result | `_finish`, `retry`, `ack_cancel`, `save_checkpoint`, `heartbeat` all test `state='running' AND lease_owner AND generation AND lease_until > now` | `valid()`, `applyLease`, `applyRetry`; rejection is `terminal` for a terminal row, otherwise `lost_lease` (`save_checkpoint`: always `lost_lease`) | `invSingleLiveLease`, `invStaleRejected`, `invGenerationMonotone` |
| worker id | `_check_lease_args` pattern | worker = small int; id syntax not modelled | Rust unit tests |
| 4.1.2 idempotency key, same key never runs twice | `UNIQUE (dedupe_key)`, `enqueue` `replayed`/`already_done`/`terminal` | `enqueue(j)` creates a row only when absent | `invAtMostOnceSuccess`, `invNoLostJob` (`rows`) |
| 4.1.3 job CAS in the same transaction as stage products | not in the job functions (caller's transaction) | out of FM1 | `jobs_live.rs` (`transition_in_tx`) |
| 4.1.4 retries, attempt cap | `retry()`: plain retry at the cap becomes `failed` (`exhausted`) | `applyRetry` | `invNoLostJob` (`attempt <= max`), `witExhausted` |
| `PARSER_BUSY` does not count an attempt | `retry(p_count_attempt=false)` gives the attempt back | `cnt=false` keeps `attempt-1` (floor 0) | `witBusyGiveBack` |
| backoff, jitter, per-kind delay cap | `Backoff`, `_retry_delay_cap_ms` | not modelled (delay 0, a queued job is always due) | KP4 (V-S4-01), `jobs_live.rs` |
| 4.1.5 cancel: queued becomes cancelled, running gets the flag, worker acks at its checkpoint | `cancel()`, `ack_cancel()` | `cancel(j)`, `leaseStep("ack_cancel")` (`not_requested` when nobody asked) | `invCancelTerminal`, `invTerminalAbsorbing`, `witCancelled` |
| 4.1.6 poison pill: `crash_count >= 2` or attempt at the cap, then quarantined | `retry(p_crash=true)`, `quarantine()` | `applyRetry` quarantine branch, `leaseStep("quarantine")` | `invQuarantineAfterCrashes`, `witQuarantined` |
| 4.1.8 checkpoint is saved only under a live lease | `save_checkpoint()` | `leaseStep("checkpoint")` (`saved` / `lost_lease`; the JSON itself is not modelled) | conformance result compare |
| 4.1.7 backpressure, priority | `claim` order by priority, run_at, id | not modelled; the conformance driver steers the pick with priority | n/a |

## 3. SQL function to model action

| Function | Model action(s) | Observable results compared by V-S2-01 |
|---|---|---|
| `enqueue` | `enqueue` | enqueued, replayed, already_done, terminal |
| `claim` | `claim`, `claimNone` | claimed, empty; reclaim effects on other jobs |
| `heartbeat` | `leaseStep heartbeat` | extended, lost_lease, terminal |
| `complete` / `fail` / `quarantine` | `leaseStep complete/fail/quarantine` | succeeded, failed, quarantined, lost_lease, terminal |
| `retry` | `leaseStep retry` with `(crash, cnt)` | retry_scheduled, exhausted, quarantined, lost_lease, terminal |
| `cancel` | `cancel` | cancelled, cancel_requested, terminal, not_found |
| `ack_cancel` | `leaseStep ack_cancel` | cancelled, not_requested, lost_lease, terminal |
| `save_checkpoint` | `leaseStep checkpoint` | saved, lost_lease |
| lease expiry (time) | `expire` | driver ages `lease_until` |
| `append_event` | FM4 (`event_stream.qnt`) | n/a |

## 4. Invariants (all in `inv`)

| Name | Statement |
|---|---|
| `invLeaseShape` | `running` iff owner set; only running jobs hold a live lease (`job_lease_shape` CHECK) |
| `invSingleLiveLease` | at most one `(worker, generation)` pair is accepted by the CAS for a job |
| `invGenerationMonotone` | generation never drops and rises only by one, only through `claim` |
| `invStaleRejected` | an operation carrying an older generation is never accepted |
| `invTerminalAbsorbing`, `invCancelTerminal` | a terminal state never changes; `cancelled` in particular |
| `invQuarantineAfterCrashes` | `crash_count` stops at 2 and 2 means quarantined |
| `invNoLostJob` | a row exists iff enqueued once; attempts stay within `[0, max]`; running implies attempt >= 1; queued holds no lease; an expired lease is always reclaimable by the next `claim` (the action is enabled in every such state) |
| `invAtMostOnceSuccess` | per dedupe key, `complete` succeeds at most once, and exactly when the row is `succeeded` |

Liveness ("every job eventually reaches a terminal state") is **not model checked** here: Apalache
through `quint verify` checks safety only and TLC is not installed. The structural ingredients are
covered (expired leases are reclaimed by `claim`, the attempt cap forces `failed`/`quarantined`, no
state has only the lease holder as exit), and `quint run --witnesses` shows every outcome is reachable.
TLC liveness stays on the `formal-verify-deep` list.

## 5. Findings while mapping (for the implementer lead)

1. **Attempt cap vs quarantine wording.** 10 section 4.1.6 says "crash_count >= 2, or attempt reaches 3,
   then `quarantined`". The implemented `retry()` quarantines at the attempt cap only when the retry
   itself is a crash (`p_crash`); a transient failure at the cap becomes `failed` (`exhausted`), and an
   expired lease at the cap becomes `failed` (`lease_expired_attempt_cap`). If the spec meant every
   parse job at the cap should be `quarantined`, the stage code (not the job functions) must call
   `quarantine`; the model follows the implementation. Model and SQL agree (conformance), the question is
   a spec reading.
2. `complete`, `fail`, `quarantine`, and `retry` ignore `cancel_requested`. A worker that never
   checks the flag completes normally. This matches 4.1.5 ("worker checks at checkpoints") and the
   model keeps it.
3. A job that is retried or whose lease expires keeps `cancel_requested = true` when it was set; the
   next claim hands the flag to the next worker, and an expired lease with the flag becomes `cancelled`
   directly. Consistent, not mentioned in the spec.
4. `save_checkpoint` answers `lost_lease` for a terminal job where `heartbeat`/`complete` answer
   `terminal`. Harmless, but an inconsistency in the result vocabulary.

## 6. Not covered

Backoff arithmetic and `run_at` due-ness (KP4), priority and fairness, claim with `limit` above 1, the
8-row reclaim cap with more than 8 expired jobs, generation overflow, input validation errors
(`OO010`), checkpoint payload content, multiple job kinds, and multi-transaction interleavings inside
PostgreSQL (row locks serialize each function; the model treats every function as atomic, which is
what `FOR UPDATE` gives). Parallel sessions against one database are exercised by `jobs_live.rs`, not
by the trace replay.
