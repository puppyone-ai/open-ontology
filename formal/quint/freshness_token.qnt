// formal: main=freshness_token inv=inv steps=40 verify-steps=8 deep-steps=9 witnesses="witCoverLag witTimeoutCompile witRequireComplete409 witFailedListed witOverlayBump witCaughtUp witPendingListed witPublishAbort" vinv="invRevGapFree invCoversMonotone invLiveCoversSound invCoversSound invCoversMax invCompileWaits invCoverageHonest invCoverageInScope invRequireComplete invOverlayAdvances"
// FM10: freshness token `drive_rev` and coverage (D54, 10 16.2; store/rev.rs, publish/hashes.rs).
// Every change of a Drive (a new file version, a correction overlay) takes the next `drive_rev` in the
// transaction that makes it. A change is terminal when its file version is published (`ok`) or failed
// for good (`fail`); a file change goes pending -> ready (indexed) -> ok, or pending -> fail.
// A publish run (begin / commit, so work that finishes meanwhile is not in the batch) writes
// `covers_drive_rev` = the largest r such that every change <= r is terminal, never less than before.
// A compile asks for `min_drive_rev` (omitted = the rev at the call): covered -> compile now; not covered
// -> wait, and after the wait either `require_complete` fails with 409 (it needs the Drive covered to the
// current rev and nothing in scope pending or failed) or the pack is compiled at the latest version with an
// honest `coverage`: `pending` and `failed` are exactly the in-scope files whose
// latest change is unfinished or failed, nothing from outside the scope (D26), and
// processed + pending + failed = in_scope (the BRIEF line "covered: 39/42").
// 3 files, 6 changes (so up to 2 corrections and 4 file versions), one publish at a time, compiles at
// any point (the wait is the timeout flag: it can only be set while the token is not yet covered).
// Not modelled: wait_ms arithmetic, event subscription latency, several Drives (the version holds the
// minimum over Drives, see formal/reviews/FM10.md), superseded/deleted bumps (terminal at creation, they
// cannot lower coverage).
// Notes and reviews: formal/reviews/FM10.md.
module freshness_token {
  pure val NFILES = 3
  pure val FILES: Set[int] = 0.to(NFILES - 1)
  pure val MAXREV = 6
  pure val REVS: Set[int] = 1.to(MAXREV)

  type Ch = { file: int, st: str }     // file -1: overlay; st: none | pending | ready | ok | fail
  type CRec = {
    used: bool, minRev: int, requireComplete: bool, timedOut: bool, res: str,
    rev: int, covered: int, scope: Set[int], pending: Set[int], failed: Set[int], processed: int, inScope: int,
  }

  var rev: int
  var chg: int -> Ch
  var ver: int
  var covers: int
  var inPub: bool
  var batch: Set[int]
  var prevRev: int
  var prevCovers: int
  var rec: CRec
  var lastAct: str

  pure val NOCH: Ch = { file: -2, st: "none" }
  pure val NOREC: CRec = { used: false, minRev: 0, requireComplete: false, timedOut: false, res: "", rev: 0, covered: 0,
                          scope: Set(), pending: Set(), failed: Set(), processed: 0, inScope: 0 }

  pure def terminal(c: Ch): bool = c.st == "ok" or c.st == "fail"
  // largest r <= rev with every change 1..r terminal
  // (written without a fold over a set: a set has no order)
  pure def prefixTerminal(cs: int -> Ch, rv: int): int =
    0.to(MAXREV).filter(r => r <= rv and REVS.forall(i => i <= r implies terminal(cs.get(i)))).fold(0, (a, r) => if (r > a) r else a)
  // the latest change of a file (highest rev), or none (0)
  pure def latest(cs: int -> Ch, rv: int, f: int): int =
    REVS.filter(i => i <= rv and cs.get(i).file == f).fold(0, (a, i) => if (i > a) i else a)

  action init = all {
    rev' = 0, chg' = REVS.mapBy(i => NOCH), ver' = 0, covers' = 0, inPub' = false, batch' = Set(),
    prevRev' = 0, prevCovers' = 0, rec' = NOREC, lastAct' = "init",
  }

  pure def keep(a: str): bool = true

  // a file version is stored (upload, targeted sync, reconcile): the next drive_rev; an older unfinished
  // change of the same file ends as superseded (terminal)
  action upload = {
    nondet f = FILES.oneOf()
    all {
      rev < MAXREV,
      chg' = REVS.mapBy(i =>
        if (i == rev + 1) { file: f, st: "pending" }
        else if (i <= rev and chg.get(i).file == f and (chg.get(i).st == "pending" or chg.get(i).st == "ready")) { file: f, st: "ok" }
        else chg.get(i)),
      rev' = rev + 1, ver' = ver, covers' = covers, inPub' = inPub, batch' = batch,
      prevRev' = rev, prevCovers' = covers, rec' = rec, lastAct' = "upload",
    }
  }

  // a person's correction (D59): an overlay change, ready for the next publish, same token
  action correct = all {
    rev < MAXREV,
    chg' = chg.set(rev + 1, { file: -1, st: "ready" }),
    rev' = rev + 1, ver' = ver, covers' = covers, inPub' = inPub, batch' = batch,
    prevRev' = rev, prevCovers' = covers, rec' = rec, lastAct' = "correct",
  }

  // the pipeline finishes a pending file change: indexed, or failed for good
  action process = {
    nondet i = REVS.oneOf()
    nondet bad = Set(true, false).oneOf()
    all {
      chg.get(i).st == "pending",
      chg' = chg.set(i, { ...chg.get(i), st: if (bad) "fail" else "ready" }),
      rev' = rev, ver' = ver, covers' = covers, inPub' = inPub, batch' = batch,
      prevRev' = rev, prevCovers' = covers, rec' = rec, lastAct' = "process",
    }
  }

  action beginPublish = all {
    not(inPub),
    REVS.exists(i => chg.get(i).st == "ready"),
    batch' = REVS.filter(i => chg.get(i).st == "ready"),
    inPub' = true,
    chg' = chg, rev' = rev, ver' = ver, covers' = covers,
    prevRev' = rev, prevCovers' = covers, rec' = rec, lastAct' = "beginPublish",
  }

  action abortPublish = all {
    inPub,
    inPub' = false, batch' = Set(),
    chg' = chg, rev' = rev, ver' = ver, covers' = covers,
    prevRev' = rev, prevCovers' = covers, rec' = rec, lastAct' = "abortPublish",
  }

  // commit: the batch (those still ready) becomes ok; covers_drive_rev from the states at commit time
  action commitPublish = {
    val c2 = REVS.mapBy(i => if (batch.contains(i) and chg.get(i).st == "ready") { ...chg.get(i), st: "ok" } else chg.get(i))
    val cv = prefixTerminal(c2, rev)
    all {
      inPub,
      chg' = c2, ver' = ver + 1, covers' = if (cv > covers) cv else covers,
      inPub' = false, batch' = Set(), rev' = rev,
      prevRev' = rev, prevCovers' = covers, rec' = rec, lastAct' = "commitPublish",
    }
  }

  // a compile: min_drive_rev (0..rev; omitted = rev), scope, require_complete, wait elapsed.
  // The compile measures coverage LIVE (the implementation does, so a file that became duplicate or
  // superseded after the last publish does not stall it): the largest terminal prefix now, which is never
  // below the stored `covers_drive_rev` of the latest version.
  action compile = {
    nondet mr = 0.to(MAXREV).oneOf()
    nondet sc = FILES.powerset().oneOf()
    nondet rc = Set(true, false).oneOf()
    nondet tmo = Set(true, false).oneOf()
    val live = prefixTerminal(chg, rev)
    val covered = live >= mr
    val exist = sc.filter(f => latest(chg, rev, f) > 0)
    val pend = exist.filter(f => chg.get(latest(chg, rev, f)).st == "pending" or chg.get(latest(chg, rev, f)).st == "ready")
    val fld = exist.filter(f => chg.get(latest(chg, rev, f)).st == "fail")
    // require_complete: the whole Drive is covered up to the current rev and nothing in scope is pending or failed
    val complete = live >= rev and pend.size() == 0 and fld.size() == 0
    all {
      mr <= rev,
      covered or tmo,
      rec' = { used: true, minRev: mr, requireComplete: rc, timedOut: not(covered), res: if (rc and not(complete)) "incomplete409" else "ok",
               rev: rev, covered: live, scope: sc, pending: pend, failed: fld, processed: exist.size() - pend.size() - fld.size(), inScope: exist.size() },
      chg' = chg, rev' = rev, ver' = ver, covers' = covers, inPub' = inPub, batch' = batch,
      prevRev' = rev, prevCovers' = covers, lastAct' = "compile",
    }
  }

  action step = any { upload, correct, process, beginPublish, abortPublish, commitPublish, compile }
  // the profile the live conformance test replays: a correction is not committed while a publish run is in
  // flight (a correction that lands during a run is the one race the live measure cannot see, see FM10.md)
  action stepLive = any { upload, all { not(inPub), correct }, process, beginPublish, abortPublish, commitPublish, compile }

  // ---- invariants ----
  // drive_rev is gap free: change i exists iff i <= rev, and it never goes down
  val invRevGapFree = REVS.forall(i => (i <= rev) == (chg.get(i).st != "none")) and rev >= prevRev and rev <= prevRev + 1
  // covers_drive_rev never goes down, never passes drive_rev, and never runs ahead of the live measure
  val invCoversMonotone = covers >= prevCovers and covers <= rev and covers <= prefixTerminal(chg, rev)
  // the live coverage a compile reports covers only terminal changes
  val invLiveCoversSound = (lastAct == "compile") implies (REVS.forall(i => i <= rec.covered implies terminal(chg.get(i))) and rec.covered >= covers)
  // covers_drive_rev only covers terminal changes
  val invCoversSound = REVS.forall(i => i <= covers implies terminal(chg.get(i)))
  // a commit writes the largest such r (when it advances)
  val invCoversMax = lastAct == "commitPublish" implies covers >= prefixTerminal(chg, rev)
  // a compile that did not wait covers the requested rev; one that waited timed out below it
  val invCompileWaits = (lastAct == "compile") implies (if (rec.timedOut) rec.covered < rec.minRev else rec.covered >= rec.minRev)
  // require_complete: success only when the Drive is covered to the current rev and nothing in scope is pending
  // or failed; the 409 only for a request that asked for it and was not complete
  val invRequireComplete = (lastAct == "compile") implies (
    ((rec.requireComplete and rec.res == "ok") implies (rec.covered >= rec.rev and rec.pending == Set() and rec.failed == Set())) and
    (rec.res == "incomplete409" implies (rec.requireComplete and (rec.covered < rec.rev or rec.pending != Set() or rec.failed != Set()))))
  // coverage is exactly the unfinished or failed in-scope files, and counts add up (BRIEF line)
  val invCoverageHonest = (lastAct == "compile") implies {
    val exist = rec.scope.filter(f => latest(chg, rev, f) > 0)
    rec.pending == exist.filter(f => chg.get(latest(chg, rev, f)).st != "ok" and chg.get(latest(chg, rev, f)).st != "fail") and
    rec.failed == exist.filter(f => chg.get(latest(chg, rev, f)).st == "fail") and
    rec.processed + rec.pending.size() + rec.failed.size() == rec.inScope and rec.inScope == exist.size() and
    rec.pending.intersect(rec.failed) == Set()
  }
  // nothing outside the scope is listed (D26)
  val invCoverageInScope = rec.pending.subseteq(rec.scope) and rec.failed.subseteq(rec.scope)
  // a correction takes a drive_rev of its own and is not covered until a publish consumed it
  val invOverlayAdvances = REVS.forall(i => (chg.get(i).file == -1 and chg.get(i).st == "ready") implies covers < i)

  val inv = invRevGapFree and invCoversMonotone and invLiveCoversSound and invCoversSound and invCoversMax and invCompileWaits and
            invCoverageHonest and invCoverageInScope and invRequireComplete and invOverlayAdvances

  // ---- witnesses ----
  val witCoverLag = covers < rev and REVS.exists(i => i > covers and i <= rev and chg.get(i).st == "ok")
  val witTimeoutCompile = lastAct == "compile" and rec.timedOut and rec.res == "ok"
  val witRequireComplete409 = lastAct == "compile" and rec.res == "incomplete409"
  val witFailedListed = lastAct == "compile" and rec.failed.size() > 0
  val witPendingListed = lastAct == "compile" and rec.pending.size() > 0
  val witOverlayBump = REVS.exists(i => chg.get(i).file == -1 and chg.get(i).st == "ok")
  val witCaughtUp = lastAct == "commitPublish" and covers == rev and rev >= 3
  val witPublishAbort = lastAct == "abortPublish"
}
