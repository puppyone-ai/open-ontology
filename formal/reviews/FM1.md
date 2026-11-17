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

## Independent review (native sub-agent, 2026-11-17)

Reviewer: native sub-agent (80 section 5 step 2), read-only except this section. Read 10 section 4.1 and
the 3.2 function table, 80 sections 2-5, `jobs.qnt`, migration `0138` (lines 332-737), `jobs.rs`,
`conformance_jobs.rs`. Ran `quint typecheck` and four quick single-mutation runs of
`quint run --max-samples=300 --max-steps=40 --seed=7` on temporary copies of the model (nothing edited).

**Verdict: APPROVE WITH CHANGES.** The model is a faithful transcription of the SQL (no guard or effect
mismatch found); one spec/SQL disagreement (finding 1) needs an owner decision before L2 sign-off.

### Q1. SQL function vs model action (line by line)

Checked against `0138`; all match, no mismatch:
- `enqueue` (332-): new row `queued`; existing queued/running `replayed`, succeeded `already_done`, else `terminal`. Matches `outcome`.
- `claim` (392-): reclaim loop (cancel flag -> `cancelled`; `attempt >= max_attempts` -> `failed`; else `queued`), then lease with attempt+1, generation+1. Matches `reclaimOne`/`claimOutcome`. The 8-row cap and `kind = ANY` are abstracted (stated in section 6).
- `heartbeat` (462-), `_finish` (497-), `ack_cancel` (681-), `save_checkpoint` (709-): CAS is `running AND owner AND generation AND lease_until > now` exactly as `valid()`; rejection vocabulary (`terminal` vs `lost_lease`, checkpoint always `lost_lease`) matches `rejection`/`applyLease`.
- `retry` (575-): crash branch `p_crash AND (crash+1 >= limit OR attempt >= max)` (lines ~626), exhausted branch `p_count_attempt AND attempt >= max` (~634), attempt give-back `greatest(attempt-1,0)`; identical to `applyRetry` including the cap test using the pre-give-back attempt.
- `cancel` (650-): queued -> `cancelled` (flag not set), running -> flag, else `terminal`, absent `not_found`. Matches.
Notes: (a) a running job whose lease expired but is not yet reclaimed accepts `cancel` (flag set) and is then reclaimed to `cancelled`; the model does the same (cancel uses `m`, reclaim happens only in claim). (b) `retry` is `SECURITY DEFINER` and `_finish` is `SECURITY INVOKER`; harmless, not a model matter.

### Q2. Invariants

Right set for 4.1: no double lease (`invSingleLiveLease`), stale generation (`invStaleRejected`, `invGenerationMonotone`), terminal/cancel absorbing, quarantine, at-most-once, no lost job. Mutation check: dropping the generation test in `valid`, dropping the attempt cap, and dropping the crash limit are each caught within 300 samples (M1, M2, M4). Weak or vacuous points:
- `invAtMostOnceSuccess` is nearly tautological: `successes` counts accepted `complete`, which needs `running`, which a succeeded job can never be again. It proves the job-row state machine, not "the stage product is committed at most once" (that depends on 4.1.3 same-transaction ordering, outside FM1; stated in the table but not in the invariant name).
- `invLeaseShape` just restates the `job_lease_shape` CHECK; `invGenerationMonotone` is structural (only `claim` touches `gen`). Fine as regression guards, but not independent evidence.
- `invNoLostJob` mixes a bookkeeping check (`rows`) with weak checks; it does not say "an expired running lease is reclaimable" (that is only the enabledness of `claim`, so a deadlock would show as no enabled step, which `quint run` does not report).
- **Gap (mutation M3 survived):** replacing the `jb.cancel` test in `ack_cancel` by `true` is NOT caught by any invariant (only by conformance). Nothing states "a job reaches `cancelled` only if it was queued at `cancel` time or had `cancel` set".
- Missing, worth adding: checkpoint monotonicity/ownership (the model keeps no checkpoint value, so a stale worker overwriting a newer checkpoint, or a checkpoint surviving reclaim, is unobservable); `crash` count never decreases; `attempt <= gen` and `gen - attempt` equals the number of given-back attempts; reclaim fairness (an expired lease is reclaimed before any younger queued job is leased: ordering is `lease_until, id` in SQL but unmodelled; with at most 8 rows per call it is a real starvation surface).
- Liveness (every job reaches a terminal state) is declared out of scope for Apalache and moved to the deep list; accepted, but it is the one headline property in 80 section 2 that nothing checks today.

### Q3. Bounds and abstractions

Reasonable and stated (sections 1 and 6): 3 workers, 4 jobs, `max_attempts` 3, crash limit 2, generation 6, delay 0, `live` boolean instead of time, one kind, atomic functions (justified by `FOR UPDATE`). Limits not stated: (a) with delay 0 the `run_at <= now` filter in `claim` is never exercised, so a bug that leaves a due job unclaimable is invisible; (b) `claim_none` is enabled only when nothing is queued, so "claim returns empty although a due job exists" cannot be generated; (c) the honest-actor bias (75%) is a `quint run` heuristic only, Apalache ranges freely; (d) worker identity cannot be reused after restart (a new worker with the same id and an old generation), which 10 4.1.1 avoids with a random startup nonce; the model assumes unique ids.

### Q4. Attempt cap vs quarantine (spec 4.1.6 vs SQL)

Reading per spec: 4.1.6 states two independent triggers, "crash_count >= 2, OR attempt reaches 3 -> quarantined", with the file going to `parse_failed` / `DOCS_PARSE_QUARANTINED`. 4.1.4 says the parse counted errors are "crash, timeout, OOM" and "3 attempts, then manual". So for the parse kind the literal reading is: a job that hits 3 counted attempts for any counted reason is `quarantined` (manual retry only), not `failed`. The SQL implements only the crash-flagged variant (`retry` crash branch) and makes timeout/OOM at the cap `failed` (`exhausted`), and an expired lease at the cap `failed` (`lease_expired_attempt_cap`; 3.2 table documents this one explicitly for `claim`). The `quarantine()` function requires a live lease, so a stage cannot repair `exhausted`/`lease_expired_attempt_cap` afterwards: the SQL result is final and the stage layer cannot reach the spec state for the parse kind. Safety (lease, generation, at-most-once) is unaffected, both are terminal absorbing states. It matters for behaviour: quarantine is per blob and inherited by other paths of the same content, `failed` is not, so a repeatedly timing-out blob keeps being re-attempted per path (3 slots x timeout each), which 4.1.6 "queue throughput" and L3 acceptance 3 want to prevent; it also changes the file code/state. Conclusion: the spec reading (cap reached -> quarantined for parse, at least when every attempt was a crash/timeout/OOM) is the intended one; the implementation is narrower. Both readings are coherent in the model; the model currently pins the narrow one.

### Q5. Conformance test

Strong where it looks: per-step result string compared to the model; per-step comparison of the full job row projection (state, attempt, generation, owner index, live flag, crash_count, cancel_requested) for all jobs including reclaim side effects on non-targeted jobs; row count; success count. `claim` is steered by priority and asserted to return the model's job. Weaknesses: (1) `successes` is counted by the driver from the same status it compares, so that check adds nothing; (2) not compared: `last_error`, `run_at`, `checkpoint`, `lease_until` (only the boolean), `max_attempts`; so the model's silence on checkpoint and error codes means a wrong `last_error` (e.g. `lease_expired_cancelled` vs `lease_expired_attempt_cap`, which is the only place the two reclaim-to-terminal outcomes are told apart by anything but state) would pass; (3) all retry delays are 0 so run_at/backoff clamping is untested here; (4) `expire` writes `lease_until` directly (fine) but there is no wall-clock lease expiry path, a lease that expires between a CAS check and its UPDATE cannot occur (single connection, no concurrency): the race between `claim` reclaim and a late `heartbeat` is covered by `jobs_live.rs`, not here; (5) the committed sample set is small; the 10,000-trace bulk run is manual, so the gate depends on someone running it (check CI wiring).

### Findings

1. **[blocking] Spec/impl conflict on cap -> quarantine (Q4).** Owner decision needed before sign-off: either amend 10 4.1.6 (cap -> `quarantined` only when the last failure was a crash; timeout/OOM at cap -> `failed`, retryable manually) and state it in 4.1.4, or change `claim` reclaim and `retry` so that for `kind = 'parse'` reaching `max_attempts` yields `quarantined` (and update `jobs.qnt` `reclaimOne`/`applyRetry` plus the test, which currently locks the narrow behaviour in). Without this, FM1 "超限即进入隔离" (80 section 3.1) is not what the implementation does.
2. **[blocking] Invariant gap: cancel provenance.** Mutating `ack_cancel` to ignore `cancel_requested` survives all invariants (M3). Add `invCancelProvenance`: `st == cancelled` implies the job was cancelled while queued or `cancel` was set (track with a history variable, e.g. `cancelFlagEver`), and make `invAtMostOnceSuccess`'s name/comment honest that it covers the row state machine only.
3. **[non-blocking] Checkpoint is unmodelled.** Add a monotone `ckpt` counter set by `checkpoint` under a valid CAS and preserved across reclaim; invariant `ckpt` never decreases and is only changed by the current lease holder.
4. **[non-blocking] Liveness unchecked.** Keep on the deep list, but add a deadlock/enabledness guard: at least one action is enabled in every reachable state, and an expired running lease always enables `claim` (the structural argument in section 4 should be an invariant check, not a comment).
5. **[non-blocking] Reclaim fairness and 8-row cap** are unmodelled (starvation surface under many expired leases); add a bound or a deep-run variant with 9+ jobs and a cap of 2.
6. **[non-blocking] Conformance depth.** Compare `last_error` (via a model `lastErr` code), `max_attempts`, and `checkpoint` presence; drop or replace the driver-side `successes` check; add one non-zero-delay scenario so `run_at <= now` is exercised; confirm the 10,000-trace run is wired into a gate.
7. **[non-blocking] Re-enqueue after terminal.** `enqueue` returns `terminal` for failed/cancelled/quarantined and never reruns (spec 3.2: caller adds `retry#n` to the key). That makes "manual retry of a quarantined blob" depend on a key convention that the model cannot see; record it as an assumption in section 6.
8. **[non-blocking] Result vocabulary** (`save_checkpoint` -> `lost_lease` on terminal, others `terminal`): either document in 10 3.2 or unify.

### Recommended changes before 80 step 3

Resolve finding 1, add the invariant of finding 2 (and re-run the M3 mutation to see it fail), state the limits in Q3 in section 6, then re-run `quint verify` and the conformance replay.

## Implementation-lead decision (coordinator, 2026-11-17)

- Blocking 1: spec 10 §4.1.6 is explicit that a parse job reaching attempt 3 is `quarantined` (manual retry only, inherited per blob). The SQL is changed to follow the spec (crash, timeout, OOM and lease-expiry caps of parse kinds all end in `quarantined`); FM1 and the conformance test are updated with it. Card: B-S3-04 (lane A). Migration 0138 has not left this branch, so it is edited in place before its first push.
- Blocking 2: add `invCancelProvenance`; rename or document `invAtMostOnceSuccess` as row-level only. Same card.
- Non-blocking items are tracked for L3 (checkpoint monotonicity, reclaim fairness, `run_at` coverage in conformance) and L4 deep checks (liveness).
- Review status: approved with changes; the L1 formal gate closes when both blocking items land and `formal-verify-fast` for FM1 is green again.

## CI bounds (card V-S1-01, time budget of 80 section 6)

`formal-verify-fast` must fit 15 minutes per (model, invariant) job. Measured with `formal/scripts/formal-verify-fast`,
one Apalache server per run, 8 runs side by side on a 16 core machine that was also running other work (load average
9 to 22), so the numbers are upper bounds:

| invariant | 10 steps | 8 steps |
|---|---|---|
| invSingleLiveLease | 25 s | n/m |
| invStaleRejected | 16 s | n/m |
| invTerminalAbsorbing | 128 s | n/m |
| invGenerationMonotone | 161 s | n/m |
| invQuarantineAfterCrashes | 241 s | n/m |
| invCancelProvenance | 305 s | 127 s |
| invCancelTerminal | 393 s | 169 s |
| invLeaseShape | 568 s | 167 s |
| invAtMostOnceSuccess | 784 s (over 12 min) | 195 s |
| invNoLostJob | still running after 40 min (30 min in an earlier run) | 586 s |

FAST bound: `verify-steps=8` for the whole model (the slowest invariant is 586 s). Nightly bound: `deep-steps=10`
(`formal-verify-deep`, own machine). `invNoLostJob` at 10 steps does not fit a PR job and stays in the deep run.
