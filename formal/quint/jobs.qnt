// formal: main=jobs inv=inv steps=40 verify-steps=10 witnesses="witSucceeded witCancelled witQuarantined witReclaimed witCapQuarantined witFailed witBusyGiveBack" vinv="invLeaseShape invSingleLiveLease invGenerationMonotone invStaleRejected invTerminalAbsorbing invCancelTerminal invCancelProvenance invQuarantineAfterCrashes invNoLostJob invAtMostOnceSuccess"
// FM1: job lease protocol, as IMPLEMENTED by crates/oo-pipeline/migrations/0138
// (_oo_docs.enqueue/claim/heartbeat/complete/fail/quarantine/retry/cancel/
// ack_cancel/save_checkpoint). Spec 10 4.1, 80 3.1 FM1. Human notes, bounds
// and the rule-by-rule mapping: formal/reviews/FM1.md.
//
// One job kind: the parse kind, the only one 10 4.1.6 quarantines. Its attempt
// cap ends in `quarantined`, never `failed`: by crash, by a counted failure
// (timeout, OOM) or by an expired lease at the cap (the SQL says so for
// kind = 'parse'; other kinds fail at the cap and are not modelled here).
// Job index = dedupe key (the table has UNIQUE(dedupe_key)).
// Time is not a number: a lease is `live` (lease_until > now) or not; the
// `expire` action is time passing. Retry delays are always 0 (a queued job is
// always due); backoff values are not modelled (KP4 covers them).
module jobs {
  pure val W = 3             // workers 0..2
  pure val J = 4             // dedupe keys / jobs 0..3
  pure val MAX_ATTEMPTS = 3  // job.max_attempts (the parse kind)
  pure val CRASH_LIMIT = 2   // CRASH_QUARANTINE_LIMIT, retry(p_crash_limit)
  pure val GMAX = 6          // model bound: no claim beyond generation 6
  pure val NONE = -1
  pure val WORKERS: Set[int] = 0.to(W - 1)
  pure val JOBS: Set[int] = 0.to(J - 1)
  pure val GENS: Set[int] = 1.to(GMAX)

  type Job = { st: str, attempt: int, gen: int, owner: int, live: bool, crash: int, cancel: bool }
  // kind: enqueue expire claim heartbeat complete fail quarantine retry cancel ack_cancel checkpoint
  type Op = { kind: str, w: int, j: int, g: int, crash: bool, cnt: bool }
  type Out = { job: Job, res: str, acc: bool }

  pure val ABSENT: Job = { st: "absent", attempt: 0, gen: 0, owner: NONE, live: false, crash: 0, cancel: false }
  pure val NOOP: Op = { kind: "init", w: 0, j: 0, g: 0, crash: false, cnt: true }

  var jobs: int -> Job
  var successes: int -> int   // times complete() returned succeeded (effect count)
  var rows: int -> int        // rows inserted per dedupe key
  var term: int -> str        // first terminal state reached ("" if none yet)
  var prevGen: int -> int     // generation before the last step
  var cancelQueued: int -> bool  // history: cancel() hit this job while it was queued
  var lastOp: Op
  var lastRes: str
  var lastAccepted: bool      // the last op changed state under a live lease
  var lastStale: bool         // the last lease op carried an older generation

  pure def isTerm(s: str): bool = s == "succeeded" or s == "failed" or s == "cancelled" or s == "quarantined"
  pure def isLeaseOp(k: str): bool =
    k == "heartbeat" or k == "complete" or k == "fail" or k == "quarantine" or k == "retry" or k == "ack_cancel" or k == "checkpoint"

  // state='running' AND lease_owner=p_owner AND generation=p_generation AND lease_until>now()
  pure def valid(jb: Job, w: int, g: int): bool = jb.st == "running" and jb.owner == w and jb.gen == g and jb.live
  // _finish / retry / heartbeat fallthrough: terminal row -> 'terminal', otherwise (incl. no row) 'lost_lease'
  pure def rejection(jb: Job): str = if (isTerm(jb.st)) "terminal" else "lost_lease"

  // claim step 1: every expired running lease goes back (cancelled / failed / queued)
  pure def reclaimOne(jb: Job): Job =
    if (jb.st == "running" and not(jb.live)) {
      if (jb.cancel) { ...jb, st: "cancelled", owner: NONE, live: false }
      else if (jb.attempt >= MAX_ATTEMPTS) { ...jb, st: "quarantined", owner: NONE, live: false }
      else { ...jb, st: "queued", owner: NONE, live: false }
    } else jb
  pure def reclaimAll(m: int -> Job): int -> Job = JOBS.mapBy(i => reclaimOne(m.get(i)))

  // retry(p_id,p_owner,p_generation,p_code,delay 0,p_count_attempt,p_crash,limit 2)
  pure def applyRetry(jb: Job, op: Op): Out = {
    val crash2 = jb.crash + (if (op.crash) 1 else 0)
    val attempt2 = if (op.cnt) jb.attempt else (if (jb.attempt - 1 > 0) jb.attempt - 1 else 0)
    if (not(valid(jb, op.w, op.g))) { job: jb, res: rejection(jb), acc: false }
    // crash limit, or the attempt cap reached by a crash or by any counted failure: quarantine
    else if ((op.crash and (crash2 >= CRASH_LIMIT or jb.attempt >= MAX_ATTEMPTS)) or (op.cnt and jb.attempt >= MAX_ATTEMPTS))
      { job: { ...jb, st: "quarantined", crash: crash2, owner: NONE, live: false }, res: "quarantined", acc: true }
    else
      { job: { ...jb, st: "queued", attempt: attempt2, crash: crash2, owner: NONE, live: false }, res: "retry_scheduled", acc: true }
  }

  pure def applyLease(jb: Job, op: Op): Out = {
    val ok = valid(jb, op.w, op.g)
    val rej: Out = { job: jb, res: rejection(jb), acc: false }
    if (op.kind == "heartbeat") (if (ok) { job: { ...jb, live: true }, res: "extended", acc: true } else rej)
    else if (op.kind == "complete") (if (ok) { job: { ...jb, st: "succeeded", owner: NONE, live: false }, res: "succeeded", acc: true } else rej)
    else if (op.kind == "fail") (if (ok) { job: { ...jb, st: "failed", owner: NONE, live: false }, res: "failed", acc: true } else rej)
    else if (op.kind == "quarantine") (if (ok) { job: { ...jb, st: "quarantined", owner: NONE, live: false }, res: "quarantined", acc: true } else rej)
    else if (op.kind == "ack_cancel")
      (if (ok) (if (jb.cancel) { job: { ...jb, st: "cancelled", owner: NONE, live: false }, res: "cancelled", acc: true }
                else { job: jb, res: "not_requested", acc: false })
       else rej)
    // save_checkpoint answers lost_lease for any failed CAS, terminal rows included
    else if (op.kind == "checkpoint") (if (ok) { job: jb, res: "saved", acc: true } else { job: jb, res: "lost_lease", acc: false })
    else applyRetry(jb, op)
  }

  // weighted choice of the operation (uniform over 16 slots; Apalache sees the same 12 kinds)
  pure def kindOf(i: int): str =
    if (i <= 1) "enqueue" else if (i == 2) "expire" else if (i <= 4) "claim" else if (i == 5) "claim_none"
    else if (i == 6) "cancel" else if (i == 7) "heartbeat" else if (i == 8) "complete" else if (i == 9) "fail"
    else if (i == 10) "quarantine" else if (i == 11) "ack_cancel" else if (i == 12) "checkpoint" else "retry"

  type Res = { jobs: int -> Job, res: str, acc: bool }

  // claim step 1 + 2 as a whole: reclaim, then lease j (when j is queued after the reclaim)
  pure def claimOutcome(rj: int -> Job, op: Op): Res = {
    val jb = rj.get(op.j)
    { jobs: rj.set(op.j, { ...jb, st: "running", attempt: jb.attempt + 1, gen: jb.gen + 1, owner: op.w, live: true }),
      res: "claimed", acc: true }
  }

  // one model step on the job table; `enabled` says when the step exists
  pure def stepEnabled(m: int -> Job, rj: int -> Job, op: Op): bool = {
    val jb = m.get(op.j)
    if (op.kind == "expire") jb.st == "running" and jb.live
    else if (op.kind == "claim") rj.get(op.j).st == "queued" and rj.get(op.j).gen < GMAX
    else if (op.kind == "claim_none") JOBS.forall(i => rj.get(i).st != "queued")
    else true
  }

  pure def outcome(m: int -> Job, rj: int -> Job, op: Op): Res = {
    val jb = m.get(op.j)
    if (op.kind == "enqueue") {
      if (jb.st == "absent") { jobs: m.set(op.j, { ...ABSENT, st: "queued" }), res: "enqueued", acc: true }
      else {
        jobs: m, acc: false,
        res: if (jb.st == "queued" or jb.st == "running") "replayed" else if (jb.st == "succeeded") "already_done" else "terminal"
      }
    }
    else if (op.kind == "expire") { jobs: m.set(op.j, { ...jb, live: false }), res: "expired", acc: true }
    else if (op.kind == "claim") claimOutcome(rj, op)
    else if (op.kind == "claim_none") { jobs: rj, res: "empty", acc: false }
    else if (op.kind == "cancel") {
      if (jb.st == "absent") { jobs: m, res: "not_found", acc: false }
      else if (jb.st == "queued") { jobs: m.set(op.j, { ...jb, st: "cancelled" }), res: "cancelled", acc: true }
      else if (jb.st == "running") { jobs: m.set(op.j, { ...jb, cancel: true }), res: "cancel_requested", acc: true }
      else { jobs: m, res: "terminal", acc: false }
    }
    else {
      val o = applyLease(jb, op)
      { jobs: m.set(op.j, o.job), res: o.res, acc: o.acc }
    }
  }

  action init = all {
    jobs' = JOBS.mapBy(i => ABSENT),
    successes' = JOBS.mapBy(i => 0),
    rows' = JOBS.mapBy(i => 0),
    cancelQueued' = JOBS.mapBy(i => false),
    term' = JOBS.mapBy(i => ""),
    prevGen' = JOBS.mapBy(i => 0),
    lastOp' = NOOP,
    lastRes' = "init",
    lastAccepted' = false,
    lastStale' = false,
  }

  // The single transition. `honest` makes the actor the current lease holder with its own
  // generation (happy and crash paths for `quint run`; redundant for Apalache, which
  // already ranges over every (worker, generation)).
  action step = {
    nondet ki = 0.to(15).oneOf()
    nondet w0 = WORKERS.oneOf()
    nondet j = JOBS.oneOf()
    nondet g0 = GENS.oneOf()
    nondet hk = 0.to(3).oneOf()
    nondet ck = 0.to(3).oneOf()
    nondet nk = 0.to(3).oneOf()
    val kind = kindOf(ki)
    val honest = hk > 0      // 75% of the time the actor is the lease holder
    val crash = ck == 0      // 25% crashes
    val cnt = nk > 0         // 75% count the attempt
    val cur = jobs.get(j)
    val w = if (honest and cur.st == "running") cur.owner else w0
    val g = if (honest and cur.st == "running") cur.gen else (if (isLeaseOp(kind)) g0 else 0)
    val op: Op = { kind: kind, w: w, j: j, g: g, crash: crash, cnt: cnt }
    val rj = reclaimAll(jobs)
    val o = outcome(jobs, rj, op)
    all {
      stepEnabled(jobs, rj, op),
      jobs' = o.jobs,
      successes' = if (kind == "complete" and o.acc) successes.set(j, successes.get(j) + 1) else successes,
      rows' = if (kind == "enqueue" and o.acc) rows.set(j, rows.get(j) + 1) else rows,
      cancelQueued' = if (kind == "cancel" and cur.st == "queued") cancelQueued.set(j, true) else cancelQueued,
      term' = JOBS.mapBy(i => if (term.get(i) == "" and isTerm(o.jobs.get(i).st)) o.jobs.get(i).st else term.get(i)),
      prevGen' = JOBS.mapBy(i => jobs.get(i).gen),
      lastOp' = op,
      lastRes' = o.res,
      lastAccepted' = o.acc,
      lastStale' = (isLeaseOp(kind) and g < cur.gen),
    }
  }

  // ---- invariants (safety) ----
  // running <=> lease owner set; only running jobs hold a live lease
  val invLeaseShape = JOBS.forall(i => {
    val jb = jobs.get(i)
    (jb.st == "running") == (jb.owner != NONE) and (jb.live implies jb.st == "running")
  })
  // no two live leases per job: at most one (worker, generation) is accepted by the CAS
  val invSingleLiveLease = JOBS.forall(i =>
    WORKERS.map(w => GENS.filter(g => valid(jobs.get(i), w, g)).size()).fold(0, (a, b) => a + b) <= 1)
  // generation never decreases and grows only by claim, by one
  val invGenerationMonotone = JOBS.forall(i =>
    jobs.get(i).gen >= prevGen.get(i) and jobs.get(i).gen <= prevGen.get(i) + 1)
  // an op carrying an older generation is never accepted
  val invStaleRejected = lastStale implies not(lastAccepted)
  // terminal states are absorbing; cancelled in particular
  val invTerminalAbsorbing = JOBS.forall(i => term.get(i) != "" implies jobs.get(i).st == term.get(i))
  val invCancelTerminal = JOBS.forall(i => jobs.get(i).st == "cancelled" implies (term.get(i) == "cancelled" and jobs.get(i).owner == NONE))
  // a job reaches `cancelled` only if cancel() found it queued or the cancel flag was set
  // (mutation M3 of formal/reviews/FM1.md: ack_cancel ignoring the flag is caught here)
  val invCancelProvenance = JOBS.forall(i =>
    jobs.get(i).st == "cancelled" implies (cancelQueued.get(i) or jobs.get(i).cancel))
  // quarantine after CRASH_LIMIT crashes (or attempt cap with a crash)
  val invQuarantineAfterCrashes = JOBS.forall(i =>
    jobs.get(i).crash <= CRASH_LIMIT and (jobs.get(i).crash >= CRASH_LIMIT implies jobs.get(i).st == "quarantined"))
  // no lost job: a row exists iff enqueued once; it is queued, leased (reclaimable once expired)
  // or terminal; attempts stay within the cap; a queued row holds no lease
  val invNoLostJob = JOBS.forall(i => {
    val jb = jobs.get(i)
    (rows.get(i) == (if (jb.st == "absent") 0 else 1)) and jb.attempt <= MAX_ATTEMPTS and jb.attempt >= 0 and
    (jb.st == "queued" implies jb.owner == NONE) and
    (jb.st == "running" implies jb.attempt >= 1)
  })
  // At most one accepted complete() per dedupe key. ROW LEVEL ONLY: it says the job row can
  // reach `succeeded` once. It does not say a stage's product is committed once: that rests
  // on 10 4.1.3 (job completion and products in one transaction), outside FM1.
  val invAtMostOnceSuccess = JOBS.forall(i =>
    successes.get(i) <= 1 and (successes.get(i) == 1) == (jobs.get(i).st == "succeeded"))

  val inv = invLeaseShape and invSingleLiveLease and invGenerationMonotone and invStaleRejected and
            invTerminalAbsorbing and invCancelTerminal and invCancelProvenance and invQuarantineAfterCrashes and invNoLostJob and invAtMostOnceSuccess

  // ---- witnesses for `quint run --witnesses` (reachability, not safety) ----
  val witSucceeded = JOBS.exists(i => jobs.get(i).st == "succeeded")
  val witCancelled = JOBS.exists(i => jobs.get(i).st == "cancelled")
  val witQuarantined = JOBS.exists(i => jobs.get(i).st == "quarantined" and jobs.get(i).crash >= CRASH_LIMIT)
  val witReclaimed = JOBS.exists(i => jobs.get(i).gen >= 2 and jobs.get(i).st == "running")
  // quarantined by the attempt cap alone (fewer than CRASH_LIMIT crashes)
  val witCapQuarantined = JOBS.exists(i => jobs.get(i).st == "quarantined" and jobs.get(i).crash < CRASH_LIMIT and jobs.get(i).attempt >= MAX_ATTEMPTS)
  // `failed` only by an explicit fail(): a permanent error, never the cap
  val witFailed = JOBS.exists(i => jobs.get(i).st == "failed")
  val witBusyGiveBack = lastRes == "retry_scheduled" and not(lastOp.cnt)
}
