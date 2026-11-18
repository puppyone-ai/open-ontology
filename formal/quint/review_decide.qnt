// formal: main=review_decide inv=inv steps=40 verify-steps=7 deep-steps=8 witnesses="witAutoPublished witQueued witHeld witBySource witBySourceSkippedLate witStaleRefused witAlreadyDecided witLlmMergeQueued witRuleCertainAuto witInUseNotice witOrphaned witOrphanKept witRejectWins" vinv="invEvidenceGate invBySourceAsOf invNoDoubleDecide invIfMatch invRevMonotone invLlmNeverAuto invInUseNotice invThreshold invRejectWins invOverlayNeverRetracted invOrphanQueued"
// FM5: batch review decisions (10 4.8, 5.3, 8; D22, D24, D51; crates/oo-docs/src/review/{queue,decide,overlay}.rs).
// A slot is one candidate: a draft fact (kind 0), an LLM entity merge (1), a supersession (2), or the
// "orphaned overlay" item (3, refers to `tgt`). It has a fact side (`fst`) and a review item side (`ist`).
// Classification when it appears (5.3): a human fact (it always carries the overlay of the person who made it) is eligible; confidence class 2 (>= auto_publish_bp)
// is eligible without a person; class 1 (>= queue floor) waits as a pending low_confidence_fact item;
// class 0 stays a draft, not queued (hold). A merge that is not rule-certain (identity attribute) and a
// supersession that is not rule-certain (D51) ALWAYS wait for a person; a rule-certain supersession runs
// at once, and when a live share used the old fact it also leaves one pending notice (D51).
// A person decides an item with the revision they saw (`If-Match`): a stale revision changes nothing
// (`conflict`), a decided item changes nothing (`already_decided`); accept/reject of a fact item writes an
// overlay (fact_assert makes it eligible, fact_reject keeps it out for good, and beats the threshold).
// "Accept by source" decides, in one step, only the pending low-confidence items of that source whose
// `seq` is <= the `as_of_seq` the caller gives; later arrivals stay pending. Facts that are eligible
// publish at the next publish run. An overlay is never retracted by the engine (D22): when the evidence
// of an asserted fact vanishes the overlay becomes `orphaned` and a pending orphan item is queued.
// 5 slots, 2 sources, any interleaving of arrivals, rescoring (revision bump), decisions, batches,
// publish runs and file deletions. Declining an orphan (withdrawing the overlay of a fact whose source is
// gone) retracts the fact: nothing keeps it any more.
// Not modelled: entity_merge execution details (FM-resolve), edit decisions, the 2 s debounce, the event
// payloads, idempotency keys, `review:undo` (not implemented).
// Notes and reviews: formal/reviews/FM5.md.
module review_decide {
  pure val NI = 5
  pure val SLOTS: Set[int] = 0.to(NI - 1)
  pure val SRC: Set[int] = Set(0, 1)
  pure val MAXREV = 3
  // kinds
  pure val FACT = 0
  pure val MERGE = 1
  pure val SUPER = 2
  pure val ORPHAN = 3

  type Slot = {
    kind: int, src: int,
    conf: int,            // confidence class 0 low, 1 mid, 2 high (>= auto_publish_bp)
    origin: str,          // llm | rule | human
    certain: bool,        // rule-certain (identity attribute, frontmatter supersedes)
    inUse: bool,          // a live share used the fact this supersession ends
    tgt: int,             // ORPHAN: the slot whose overlay lost its evidence
    fst: str,             // none | draft | eligible | published | rejected | retracted
    ist: str,             // none | pending | accepted | rejected | superseded (its fact was corrected away, 4.8)
    rev: int, seq: int, decs: int, via: str, asOf: int,
    executed: bool,       // merge / supersession took effect
    notice: bool,         // the D51 notice for an in-use supersession exists
    ov: str,              // none | assert | reject | active | orphaned | withdrawn
    evid: bool,           // some live evidence row remains (DOC-G2 needs one to publish)
  }

  var slots: int -> Slot
  var prev: int -> Slot
  var evSeq: int
  var nxt: int
  var last: { act: str, ok: str, i: int, rev: int, src: int, asOf: int }

  pure val EMPTY: Slot = { kind: 0, src: 0, conf: 0, origin: "llm", certain: false, inUse: false, tgt: -1, fst: "none", ist: "none",
                           rev: 0, seq: 0, decs: 0, via: "none", asOf: 0, executed: false, notice: false, ov: "none", evid: true }
  pure val NOLAST = { act: "init", ok: "", i: 0, rev: 0, src: 0, asOf: 0 }

  action init = all { slots' = SLOTS.mapBy(i => EMPTY), prev' = SLOTS.mapBy(i => EMPTY), evSeq' = 0, nxt' = 0, last' = NOLAST }

  // classification of a new candidate (5.3, D51)
  action appear = {
    nondet kd0 = Set(FACT, MERGE, SUPER).oneOf()
    appearKind(kd0)
  }
  action appearKind(kd: int): bool = {
    nondet sr = SRC.oneOf()
    nondet cf = 0.to(2).oneOf()
    nondet og = Set("llm", "rule", "human").oneOf()
    nondet ce = Set(true, false).oneOf()
    nondet iu = Set(true, false).oneOf()
    val certain = ce and og == "rule" and kd != FACT
    val s0 = { ...EMPTY, kind: kd, src: sr, conf: cf, origin: og, certain: certain, inUse: iu and kd == SUPER, seq: evSeq + 1 }
    val s1 =
      if (kd == FACT)
        (if (og == "human") { ...s0, fst: "eligible", ov: "assert" }      // a person's fact always comes with its overlay
         else if (cf == 2) { ...s0, fst: "eligible" }
         else if (cf == 1) { ...s0, fst: "draft", ist: "pending" }
         else { ...s0, fst: "draft" })
      else if (certain) { ...s0, fst: "published", executed: true, notice: s0.inUse, ist: if (s0.inUse) "pending" else "none" }
      else { ...s0, fst: "draft", ist: "pending" }
    all {
      nxt < NI,
      slots' = slots.set(nxt, s1),
      nxt' = nxt + 1, evSeq' = evSeq + 1, prev' = slots, last' = { ...NOLAST, act: "appear" },
    }
  }

  // an in-flight change of an item (new evidence, a rescore): revision + 1 while it is pending
  action rescore = {
    nondet i = SLOTS.oneOf()
    all {
      slots.get(i).ist == "pending", slots.get(i).rev < MAXREV,
      slots' = slots.set(i, { ...slots.get(i), rev: slots.get(i).rev + 1 }),
      nxt' = nxt, evSeq' = evSeq, prev' = slots, last' = { ...NOLAST, act: "rescore" },
    }
  }

  // the effect of a decision on a slot (accept / reject), per kind
  pure def effect(s: Slot, accept: bool, via: str, asOf: int): Slot = {
    val s1 = { ...s, ist: if (accept) "accepted" else "rejected", rev: s.rev + 1, decs: s.decs + 1, via: via, asOf: asOf }
    if (s.kind == FACT)
      (if (accept) { ...s1, fst: "eligible", ov: "assert" } else { ...s1, fst: "rejected", ov: "reject" })
    else if (s.kind == ORPHAN) s1
    else (if (accept) { ...s1, executed: true, fst: "published" } else s1)
  }

  // POST review:decide, one item with the revision the caller saw
  action decide = {
    nondet i = SLOTS.oneOf()
    nondet rv = 0.to(MAXREV).oneOf()
    nondet acc = Set(true, false).oneOf()
    val s = slots.get(i)
    val res = if (s.ist == "none") "not_found" else if (s.ist != "pending") "already_decided" else if (s.rev != rv) "conflict" else "ok"
    all {
      slots' = if (res == "ok") orphanFix(slots.set(i, effect(s, acc, "item", 0)), i, acc) else slots,
      nxt' = nxt, evSeq' = evSeq, prev' = slots, last' = { act: "decide", ok: res, i: i, rev: rv, src: 0, asOf: 0 },
    }
  }
  // deciding an orphan item settles the overlay of its target: accept keeps it (active), reject withdraws it
  pure def orphanFix(m: int -> Slot, i: int, acc: bool): int -> Slot = {
    val s = m.get(i)
    if (s.kind != ORPHAN) m
    else m.set(s.tgt, { ...m.get(s.tgt), ov: if (acc) "active" else "withdrawn",
                        fst: if (acc) (if (m.get(s.tgt).fst == "draft") "eligible" else m.get(s.tgt).fst) else "retracted" })
  }

  // accept or reject by source: every pending low-confidence fact item of the source created at or before as_of_seq
  action bySource = {
    nondet sr = SRC.oneOf()
    nondet ao = 0.to(NI).oneOf()
    nondet acc = Set(true, false).oneOf()
    all {
      ao <= evSeq,
      slots' = SLOTS.mapBy(i => {
        val s = slots.get(i)
        if (s.kind == FACT and s.ist == "pending" and s.src == sr and s.seq <= ao) effect(s, acc, "source", ao) else s
      }),
      nxt' = nxt, evSeq' = evSeq, prev' = slots, last' = { act: "bySource", ok: "ok", i: 0, rev: 0, src: sr, asOf: ao },
    }
  }

  // a publish run: eligible facts enter the version
  action publish = all {
    SLOTS.exists(i => slots.get(i).fst == "eligible" and slots.get(i).evid),
    // DOC-G2: a fact without any live evidence is blocked (an overlay fact waits as an orphan instead)
    slots' = SLOTS.mapBy(i => if (slots.get(i).fst == "eligible" and slots.get(i).evid) { ...slots.get(i), fst: "published" } else slots.get(i)),
    nxt' = nxt, evSeq' = evSeq, prev' = slots, last' = { ...NOLAST, act: "publish" },
  }

  // a source file is deleted: the evidence of every fact it carries is withdrawn (evidence belongs to the
  // file version, not to the fact). A fact that carries a person's overlay is never retracted (D22): its
  // overlay turns orphaned and one pending orphan item is queued per such fact; every other live fact is
  // retracted, and its pending item is retired as superseded (4.8).
  action evidenceGone = {
    nondet sr = SRC.oneOf()
    val affected = SLOTS.filter(i => slots.get(i).kind == FACT and slots.get(i).src == sr and
      (slots.get(i).fst == "draft" or slots.get(i).fst == "eligible" or slots.get(i).fst == "published"))
    val withOv = affected.filter(i => slots.get(i).ov == "assert" or slots.get(i).ov == "active")
    val k = withOv.size()
    // the orphan slot j (nxt + j) belongs to the j-th overlay fact in id order
    pure def rankOf(ws: Set[int], i: int): int = ws.filter(x => x < i).size()
    all {
      affected.size() > 0,
      nxt + k <= NI,
      slots' = SLOTS.mapBy(i => {
        val s = slots.get(i)
        // the overlay is on hold: an eligible fact that only the overlay carried (below the auto threshold) waits again
        if (withOv.contains(i)) { ...s, ov: "orphaned", evid: false, fst: if (s.fst == "eligible" and s.conf < 2 and s.origin != "human") "draft" else s.fst }
        else if (affected.contains(i) and s.ov != "orphaned")
          { ...s, fst: "retracted", evid: false, ist: if (s.ist == "pending") "superseded" else s.ist, rev: if (s.ist == "pending") s.rev + 1 else s.rev }
        else if (i >= nxt and i < nxt + k) {
          val t = withOv.filter(w => rankOf(withOv, w) == i - nxt).fold(0, (a, w) => w)
          { ...EMPTY, kind: ORPHAN, tgt: t, src: sr, fst: "none", ist: "pending", seq: evSeq + 1 }
        }
        else s
      }),
      nxt' = nxt + k, evSeq' = if (k > 0) evSeq + 1 else evSeq, prev' = slots, last' = { ...NOLAST, act: "evidenceGone", src: sr },
    }
  }

  // "mark as wrong" (D59, facts/{id}:correct): a person's fact_reject overlay on a live fact; it beats the
  // threshold and an earlier assertion, a published fact is taken out by the next version and the pending
  // item of the fact is retired as `superseded` (4.8), which is not a decision
  action markWrong = {
    nondet t = SLOTS.oneOf()
    val s = slots.get(t)
    all {
      s.kind == FACT, s.fst == "draft" or s.fst == "eligible" or s.fst == "published",
      s.ov == "none" or s.ov == "assert" or s.ov == "active",
      slots' = slots.set(t, { ...s, ov: "reject", fst: if (s.fst == "published") "retracted" else "rejected", ist: if (s.ist == "pending") "superseded" else s.ist,
                              rev: if (s.ist == "pending") s.rev + 1 else s.rev }),
      nxt' = nxt, evSeq' = evSeq, prev' = slots, last' = { ...NOLAST, act: "markWrong" },
    }
  }

  action step = any { appear, rescore, decide, bySource, publish, evidenceGone, markWrong }
  // the profile the live conformance test replays: fact candidates, orphans, batches and corrections
  // (merges and supersessions run through the resolver and temporal tests, FM5 only checks their queueing here)
  action stepFacts = any { appearKind(FACT), rescore, decide, bySource, publish, evidenceGone, markWrong }

  // ---- invariants ----
  // accept by source only touches items that existed at as_of_seq
  val invBySourceAsOf = SLOTS.forall(i => slots.get(i).via == "source" implies (slots.get(i).seq <= slots.get(i).asOf and slots.get(i).kind == FACT))
  // an item is decided at most once, and a decided item never changes again
  val invNoDoubleDecide = SLOTS.forall(i => slots.get(i).decs <= 1 and
    (((prev.get(i).ist == "accepted" or prev.get(i).ist == "rejected")) implies
      (slots.get(i).ist == prev.get(i).ist and slots.get(i).rev == prev.get(i).rev and slots.get(i).decs == prev.get(i).decs)))
  // If-Match: a stale or repeated decision changes nothing
  val invIfMatch = (last.act == "decide" and last.ok != "ok") implies slots == prev
  // the revision only grows, by one, and a pending item is decided only at its own revision
  val invRevMonotone = SLOTS.forall(i => slots.get(i).rev >= prev.get(i).rev and slots.get(i).rev <= prev.get(i).rev + 1) and
    ((last.act == "decide" and last.ok == "ok") implies slots.get(last.i).rev == last.rev + 1)
  // D51: a merge or supersession that is not rule-certain only runs after a person accepted it
  val invLlmNeverAuto = SLOTS.forall(i => ((slots.get(i).kind == MERGE or slots.get(i).kind == SUPER) and slots.get(i).executed) implies
    ((slots.get(i).origin == "rule" and slots.get(i).certain) or (slots.get(i).ist == "accepted" and slots.get(i).via == "item")))
  // D51: a rule-certain supersession of a fact a live share used always leaves its notice
  // (a supersession a person accepted from the queue was itself the question)
  val invInUseNotice = SLOTS.forall(i => (slots.get(i).kind == SUPER and slots.get(i).executed and slots.get(i).inUse) implies
    (slots.get(i).notice or slots.get(i).ist == "accepted"))
  // D24 threshold: a fact reaches the version without a person only with class 2 (or a human origin);
  // class 0 never publishes by itself; a queued one waits for its decision
  val invThreshold = SLOTS.forall(i => {
    val s = slots.get(i)
    (s.kind == FACT and (s.fst == "eligible" or s.fst == "published")) implies
      (s.conf == 2 or s.ov == "assert" or s.ov == "active" or s.ov == "orphaned")
  }) and SLOTS.forall(i => (slots.get(i).kind == FACT and slots.get(i).ist == "pending") implies (slots.get(i).fst == "draft" and slots.get(i).conf == 1))
  // fact_reject wins over everything, including a high confidence: such a fact is never eligible or published
  val invRejectWins = SLOTS.forall(i => slots.get(i).ov == "reject" implies (slots.get(i).fst == "rejected" or slots.get(i).fst == "retracted"))
  // D22: the engine never retracts or rejects a fact that carries a person's overlay
  val invOverlayNeverRetracted = SLOTS.forall(i => (slots.get(i).ov == "assert" or slots.get(i).ov == "active" or slots.get(i).ov == "orphaned") implies
    (slots.get(i).fst != "retracted" and slots.get(i).fst != "rejected"))
  // an orphaned overlay always has its pending (or decided) orphan item
  val invOrphanQueued = SLOTS.forall(i => slots.get(i).ov == "orphaned" implies
    SLOTS.exists(j => slots.get(j).kind == ORPHAN and slots.get(j).tgt == i and slots.get(j).ist == "pending"))

  // DOC-G2: a publish run leaves an eligible fact without live evidence where it was
  val invEvidenceGate = last.act == "publish" implies SLOTS.forall(i => (prev.get(i).fst == "eligible" and not(prev.get(i).evid)) implies slots.get(i).fst == "eligible")

  val inv = invEvidenceGate and invBySourceAsOf and invNoDoubleDecide and invIfMatch and invRevMonotone and invLlmNeverAuto and invInUseNotice and
            invThreshold and invRejectWins and invOverlayNeverRetracted and invOrphanQueued

  // ---- witnesses ----
  val witAutoPublished = SLOTS.exists(i => slots.get(i).kind == FACT and slots.get(i).fst == "published" and slots.get(i).conf == 2 and slots.get(i).ov == "none")
  val witQueued = SLOTS.exists(i => slots.get(i).ist == "pending" and slots.get(i).kind == FACT)
  val witHeld = SLOTS.exists(i => slots.get(i).kind == FACT and slots.get(i).fst == "draft" and slots.get(i).ist == "none" and slots.get(i).conf == 0)
  val witBySource = last.act == "bySource" and SLOTS.exists(i => slots.get(i).via == "source")
  val witBySourceSkippedLate = last.act == "bySource" and SLOTS.exists(i => slots.get(i).kind == FACT and slots.get(i).ist == "pending" and slots.get(i).src == last.src and slots.get(i).seq > last.asOf)
  val witStaleRefused = last.act == "decide" and last.ok == "conflict"
  val witAlreadyDecided = last.act == "decide" and last.ok == "already_decided"
  val witLlmMergeQueued = SLOTS.exists(i => slots.get(i).kind == MERGE and slots.get(i).ist == "pending")
  val witRuleCertainAuto = SLOTS.exists(i => slots.get(i).kind == SUPER and slots.get(i).certain and slots.get(i).executed)
  val witInUseNotice = SLOTS.exists(i => slots.get(i).kind == SUPER and slots.get(i).executed and slots.get(i).inUse and slots.get(i).notice)
  val witOrphaned = SLOTS.exists(i => slots.get(i).ov == "orphaned")
  val witOrphanKept = SLOTS.exists(i => slots.get(i).ov == "active")
  val witRejectWins = SLOTS.exists(i => slots.get(i).ov == "reject" and slots.get(i).conf == 2)
}
