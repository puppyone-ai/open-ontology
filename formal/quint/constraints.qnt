// formal: main=constraints inv=inv steps=30 verify-steps=5 deep-steps=6 witnesses="witHeld witPartial witCertainSupersede witResolved witIdentityHeld witRejectedAdopted witReleasedAfterRetract witPackWithConflict" vinv="invNoCurrentViolation invHeldHaveConflict invPartialBlocking invHeldHaveViolation invCertainNotViolation invPackNeverHeld invWellFormed"
// FM11: integrity constraints over time slices (D57, 10 16.7, publish/constraints.rs).
// One functional-dependency shape covers the four core constraints (kind 0 project_single_owner:
// key = project, val = owner; kind 1 decision_single_status: key = decision, val = status;
// kind 2 identity_unique_person: key = identity value, val = person, not time dependent;
// kind 3 rejected_not_adopted: key = alternative, val 0 = rejects / 1 = decides). Two facts of the
// same (kind, key) with different values conflict when their validity intervals [from, to) overlap
// (kind 2: always). Time has 4 slices (0..3), to = 4 means open ended.
// The publish step is the greedy check of the implementation: the incoming facts are taken one at
// a time against everything already published (including the ones accepted earlier in the same run);
// a fact that conflicts is held (stays out of the version, its (kind, key) gets an open violation);
// every other fact publishes (partial blocking). A rule-certain supersession (D51) first ends the
// older value at the new value's `from` and is not a violation. A person resolves a violation by
// naming the winner (overlay): published conflicting facts are retracted, conflicting held facts
// rejected. Held facts are re-evaluated by the next publish (they are still drafts).
// Not modelled: which of two equal candidates the temporal pass prefers (confidence, created order:
// the id order stands for "any order"), review revisions (FM5), the BRIEF text (a pack here is the
// set of published fact ids plus the open violation keys).
// Notes and reviews: formal/reviews/FM11.md.
module constraints {
  pure val NF = 6                       // fact slots
  pure val IDS: Set[int] = 0.to(NF - 1)
  pure val KINDS: Set[int] = 0.to(3)
  pure val KEYS: Set[int] = 0.to(1)
  pure val VALS: Set[int] = 0.to(2)
  pure val TMAX = 4
  pure val SLICES: Set[int] = 0.to(TMAX - 1)

  type F = {
    kind: int, key: int, v: int, lo: int, hi: int,
    certain: bool,        // rule-certain `supersedes` (D51)
    st: str,              // absent | draft | held | published | retracted | rejected
    supBy: int,           // id of the fact whose supersession ended this one (-1: none)
    confl: Set[int],       // ids it conflicted with when it was held
    ban: bool,            // a person decided against this content (resolve loser): the same content is rejected on arrival (DOC-G4)
  }
  type Key = { kind: int, key: int }

  var facts: int -> F
  var nxt: int
  var viol: Set[Key]         // open constraint_violation rows
  var pack: Set[int]         // ids in the last compiled pack
  var packConflicts: Set[Key]// "conflict pending" entities of that pack
  var lastAct: str

  pure val ABSENT: F = { kind: 0, key: 0, v: 0, lo: 0, hi: 1, certain: false, st: "absent", supBy: -1, confl: Set(), ban: false }

  pure def overlap(a: F, b: F): bool = a.lo < b.hi and b.lo < a.hi
  pure def conflict(a: F, b: F): bool =
    a.kind == b.kind and a.key == b.key and a.v != b.v and (a.kind == 2 or overlap(a, b))
  // rule-certain supersession (D51): the value that starts later and is rule-certain ends the earlier one at
  // its own start, whichever of the two arrives first; equal starts are a conflict. Only the functional
  // constraints (owner, status) are superseded that way: a rejects/decides overlap (kind 3) is always a violation
  // for a person to settle, and kind 2 is not temporal
  pure def supersedes(d: F, a: F): bool =
    d.certain and (d.kind == 0 or d.kind == 1) and a.kind == d.kind and a.key == d.key and a.v != d.v and
    overlap(a, d) and a.lo < d.lo

  action init = all {
    facts' = IDS.mapBy(i => ABSENT), nxt' = 0, viol' = Set(), pack' = Set(), packConflicts' = Set(), lastAct' = "init",
  }

  // a draft fact appears (extraction, human overlay assertion, rule-certain supersedes)
  action addDraft = {
    nondet kd0 = KINDS.oneOf()
    addDraftK(kd0)
  }
  action addDraftK(kd: int): bool = {
    nondet ky = KEYS.oneOf()
    nondet vl = VALS.oneOf()
    nondet fr = 0.to(TMAX - 1).oneOf()
    nondet tt = 1.to(TMAX).oneOf()
    nondet ce = Set(true, false).oneOf()
    all {
      nxt < NF,
      tt > fr,
      facts' = facts.set(nxt, { kind: kd, key: ky, v: if (kd == 3) vl % 2 else vl, lo: fr, hi: tt,
                                 certain: ce and kd != 2, st: "draft", supBy: -1, confl: Set(), ban: false }),
      nxt' = nxt + 1, viol' = viol, pack' = pack, packConflicts' = packConflicts, lastAct' = "addDraft",
    }
  }

  // One publish run: the incoming facts (drafts and still-held ones) in the order of the temporal pass, by
  // start slice and then fact id (a published fact is always met first, higher confidence = lower id wins
  // an equal start), each against the facts published so far (this run's acceptances included).
  // `foldl` over a list: the order is part of the model, not an accident of set iteration.
  pure val ROUNDS: List[int] = [0, 1, 2, 3, 4, 5]
  pure def publishRun(fs: int -> F, vs: Set[Key]): { fs: int -> F, vs: Set[Key] } = {
    val cand = IDS.filter(i => fs.get(i).st == "draft" or fs.get(i).st == "held")
    pure def keyOf(m: int -> F, i: int): int = m.get(i).lo * 100 + i
    ROUNDS.foldl({ fs: fs, vs: vs }, (acc, r) => {
      val pick = cand.filter(i => cand.filter(o => keyOf(fs, o) < keyOf(fs, i)).size() == r)
      if (pick.size() == 0) acc
      else {
        val d = pick.fold(-1, (a, i) => i)
        val f = acc.fs.get(d)
        val banned = IDS.exists(o => acc.fs.get(o).ban and acc.fs.get(o).kind == f.kind and acc.fs.get(o).key == f.key and
                                     acc.fs.get(o).v == f.v and acc.fs.get(o).lo == f.lo)
        val pubs = IDS.filter(o => acc.fs.get(o).st == "published")
        val supOld = pubs.filter(o => supersedes(f, acc.fs.get(o)))     // f is the newer, rule-certain value: it ends o
        val supNew = pubs.filter(o => supersedes(acc.fs.get(o), f))     // o is the newer, rule-certain value: it ends f
        val bad = pubs.filter(o => conflict(f, acc.fs.get(o)) and not(supOld.contains(o)) and not(supNew.contains(o)))
        if (banned) { fs: acc.fs.set(d, { ...f, st: "rejected", confl: Set() }), vs: acc.vs }
        else if (bad.size() == 0) {
          // f is cut where the earliest certain newer value starts (a backfilled older value is a history row)
          val cut = supNew.fold(f.hi, (m, o) => if (acc.fs.get(o).lo < m) acc.fs.get(o).lo else m)
          val by = supNew.filter(o => acc.fs.get(o).lo == cut).fold(-1, (m, o) => o)
          val fs2 = acc.fs.set(d, { ...f, st: "published", confl: Set(), hi: cut, supBy: if (supNew.size() > 0) by else f.supBy })
          { fs: IDS.mapBy(o => if (supOld.contains(o)) { ...fs2.get(o), hi: f.lo, supBy: d } else fs2.get(o)), vs: acc.vs }
        } else
          { fs: acc.fs.set(d, { ...f, st: "held", confl: bad }), vs: acc.vs.union(Set({ kind: f.kind, key: f.key })) }
      }
    })
  }

  action publish = {
    val r = publishRun(facts, viol)
    all {
      IDS.exists(i => facts.get(i).st == "draft" or facts.get(i).st == "held"),
      facts' = r.fs, viol' = r.vs, nxt' = nxt, pack' = pack, packConflicts' = packConflicts, lastAct' = "publish",
    }
  }

  // the source of a published fact is deleted: its assertion is retracted
  action retract = {
    nondet i = IDS.oneOf()
    all {
      facts.get(i).st == "published",
      facts' = facts.set(i, { ...facts.get(i), st: "retracted" }),
      nxt' = nxt, viol' = viol, pack' = pack, packConflicts' = packConflicts, lastAct' = "retract",
    }
  }

  // a person names the winner of an open violation (overlay fact_assert: it publishes in the next run); the
  // facts it conflicts with are retracted (published) or rejected (waiting)
  action resolve = {
    nondet w = IDS.oneOf()
    val fw = facts.get(w)
    all {
      fw.st == "held",
      viol.contains({ kind: fw.kind, key: fw.key }),
      facts' = IDS.mapBy(o => {
        val fo = facts.get(o)
        if (o == w) { ...fo, st: "draft", confl: Set() }       // asserted by the person; the next publish run takes it
        else if (conflict(fw, fo) and fo.st == "published") { ...fo, st: "retracted", ban: true }
        else if (conflict(fw, fo) and (fo.st == "held" or fo.st == "draft")) { ...fo, st: "rejected", ban: true }
        // held facts of the key that do not conflict with the winner wait for the next run again
        else if (fo.st == "held" and fo.kind == fw.kind and fo.key == fw.key) { ...fo, st: "draft", confl: Set() }
        else fo
      }),
      viol' = viol.exclude(Set({ kind: fw.kind, key: fw.key })),
      nxt' = nxt, pack' = pack, packConflicts' = packConflicts, lastAct' = "resolve",
    }
  }

  // compile a pack: the published facts, and "conflict pending" for every open violation (7.3, 16.7)
  action compilePack = all {
    pack' = IDS.filter(i => facts.get(i).st == "published"),
    packConflicts' = viol,
    facts' = facts, nxt' = nxt, viol' = viol, lastAct' = "compilePack",
  }

  action step = any { addDraft, publish, retract, resolve, compilePack }
  // the profile the live conformance test replays: the three time dependent constraints (the identity
  // constraint concerns Person entities and is exercised by the resolver tests and constraints_live.rs)
  action stepLive = any { all { addDraftK(0) }, all { addDraftK(1) }, all { addDraftK(3) }, publish, retract, resolve, compilePack }

  // ---- invariants ----
  pure def pubs(fs: int -> F): Set[F] = IDS.filter(i => fs.get(i).st == "published").map(i => fs.get(i))
  // restated by time slice, independent of `conflict`: at every slice one value per (kind, key);
  // identity constraints over the whole history
  val invNoCurrentViolation = KINDS.forall(k => KEYS.forall(ky => {
    val ps = pubs(facts).filter(f => f.kind == k and f.key == ky)
    if (k == 2) ps.map(f => f.v).size() <= 1
    else SLICES.forall(t => ps.filter(f => f.lo <= t and t < f.hi).map(f => f.v).size() <= 1)
  }))
  // a held fact conflicted with something real: partial blocking never holds an uninvolved fact
  val invHeldHaveConflict = IDS.forall(i => {
    val f = facts.get(i)
    f.st == "held" implies (f.confl.size() > 0 and f.confl.forall(o => facts.get(o).kind == f.kind and facts.get(o).key == f.key and facts.get(o).v != f.v))
  })
  // a publish run leaves no draft behind: everything is published or held for a reason
  val invPartialBlocking = lastAct == "publish" implies IDS.forall(i => facts.get(i).st != "draft")
  // every held fact has its open violation
  val invHeldHaveViolation = IDS.forall(i => facts.get(i).st == "held" implies viol.contains({ kind: facts.get(i).kind, key: facts.get(i).key }))
  // a rule-certain supersession ended the older value exactly where the newer starts: no overlap
  val invCertainNotViolation = IDS.forall(i => {
    val a = facts.get(i)
    a.supBy >= 0 implies (facts.get(a.supBy).certain and facts.get(a.supBy).lo == a.hi and a.kind != 2 and a.hi > a.lo)
  })
  // a pack lists only facts that are (or were) published; never a held, draft or rejected one
  val invPackNeverHeld = pack.forall(i => facts.get(i).st == "published" or facts.get(i).st == "retracted")
  val invWellFormed = IDS.forall(i => facts.get(i).st == "absent" or (facts.get(i).lo < facts.get(i).hi and facts.get(i).hi <= TMAX))

  val inv = invNoCurrentViolation and invHeldHaveConflict and invPartialBlocking and invHeldHaveViolation and
            invCertainNotViolation and invPackNeverHeld and invWellFormed

  // ---- witnesses ----
  val witHeld = IDS.exists(i => facts.get(i).st == "held")
  // a conflict is held while an unrelated fact of the same run publishes
  val witPartial = lastAct == "publish" and IDS.exists(i => facts.get(i).st == "held") and IDS.exists(i => facts.get(i).st == "published" and facts.get(i).supBy == -1 and not(IDS.exists(o => facts.get(o).st == "held" and facts.get(o).key == facts.get(i).key and facts.get(o).kind == facts.get(i).kind)))
  val witCertainSupersede = IDS.exists(i => facts.get(i).supBy >= 0)
  val witResolved = lastAct == "resolve"
  val witIdentityHeld = IDS.exists(i => facts.get(i).st == "held" and facts.get(i).kind == 2)
  val witRejectedAdopted = IDS.exists(i => facts.get(i).st == "held" and facts.get(i).kind == 3)
  val witReleasedAfterRetract = lastAct == "publish" and IDS.exists(i => facts.get(i).st == "retracted") and IDS.exists(i => facts.get(i).st == "published" and facts.get(i).confl == Set() and viol.contains({ kind: facts.get(i).kind, key: facts.get(i).key }))
  val witPackWithConflict = packConflicts.size() > 0 and pack.size() > 0
}
