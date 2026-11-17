// formal: main=sync_reconcile inv=inv steps=40 verify-steps=8 deep-steps=10 witnesses="witRename witHeld witConfirmedSweep witPartialKeepsDeleted witTargetedCreated witRecreatedDuringListing witRetrySameGeneration" vinv="invOnlySweepKills invSweepOnlyUnseen invGenerationMonotone invHeldAfterMassOnly invRenameCarries invConverged"
// FM3 (fs): incremental sync of one Drive against a folder, as implemented by
// crates/oo-docs/src/ingest/reconcile.rs (10 4.3.4, 4.3.5; D46, D47). Spec 80
// 3.1 FM3: partial listings never sweep, targeted sync never sweeps, a listing
// that is overtaken by writes does not delete what was written, a quiet
// folder converges after one complete reconcile, rename is detected by
// content hash, a mass delete is held until confirmed. Notes and bounds:
// formal/reviews/FM3.md.
//
// A full sync is two steps, so writes can fall between them: `startSync`
// lists (a snapshot of the folder; a partial listing shows only the first
// `cut` keys and says `complete = false`), `finishSync` applies the snapshot
// to the file rows and, for a complete listing, sweeps the rows that no
// listing saw (`seen < g`). Hash jobs are one action (`hashAll`), as the
// conformance driver drains them. Time is not modelled.
//
// RETRY_RESETS_MARKS = false models the code before fix FM3-001: a retried
// generation (after a failed or held attempt) kept the `seen = g` marks of
// the earlier attempt, so a file deleted in between was never swept by the
// retry. With `true` a retry first forgets the marks of its generation.
module sync_reconcile {
  pure val K = 4              // keys 0..3 (sorted path order = key order)
  pure val C = 3              // contents 1..3 (0 = no object)
  pure val MAXW = 6           // source writes, removals and moves
  pure val MAXP = 1           // partial listings
  pure val MAXT = 2           // targeted syncs / watcher hints
  pure val MASSMIN = 1        // IngestLimits.mass_delete_min of the test configuration
  pure val RETRY_RESETS_MARKS = true
  pure val KEYS: Set[int] = 0.to(K - 1)

  // st: 0 no row, 1 live, 2 deleted. seen: last_seen_generation. c: content of the current
  // version (0 = never hashed). hp: a hash job is pending.
  type Row = { st: int, seen: int, c: int, hp: bool, v: int }
  type Ls = { active: bool, g: int, snap: int -> int, snapV: int -> int, complete: bool, confirm: bool }
  pure val NOROW: Row = { st: 0, seen: 0, c: 0, hp: false, v: 0 }

  var src: int -> int         // the folder: key -> content, 0 = absent
  var rows: int -> Row        // _oo_docs.file
  var gen: int                // drive.sync_generation (moves only on a complete sync)
  var maxg: int               // highest generation any sync started (drive_sync)
  var ls: Ls                  // the listing in progress
  var held: bool              // the last attempt of generation gen+1 was a held mass delete
  var attempted: bool         // a sync of generation gen+1 has run before (retry)
  var nW: int
  var nP: int
  var nT: int
  var quiet: bool             // ghost: a complete, unheld sync saw the folder as it is, no write since
  var wSince: bool            // ghost: a source write after the current listing started
  var lastKind: str           // ghost: init write start partial held sweep targeted hash
  var shrunk: bool            // ghost: some live row stopped being live in the last step
  var sweptSeenOk: bool       // ghost: every row swept had not been seen by generation g
  var renameOk: bool          // ghost: every rename pairs an unseen old row with a new row of equal content
  var prevGen: int
  var preDeleted: bool        // ghost: a key was deleted from the folder while its row was marked by this generation
  var recreated: bool         // ghost witness: a key written again after the snapshot that lacked it
  var ver: int -> int         // identity of the object at this key (fingerprint): changes with every write or move-in

  pure def countLive(m: int -> Row): int = KEYS.fold(0, (a, k) => if (m.get(k).st == 1) a + 1 else a)
  pure def mx(a: int, b: int): int = if (a > b) a else b
  pure def allZero(m: int -> int): bool = KEYS.forall(k => m.get(k) == 0)

  // listing applied to the file rows (sync_core, first loop)
  pure def upsert(rs: int -> Row, snap: int -> int, snapV: int -> int, g: int): int -> Row =
    KEYS.mapBy(k => {
      val r = rs.get(k)
      val s = snap.get(k)
      if (s == 0) r
      else if (r.st == 0) { st: 1, seen: g, c: 0, hp: true, v: 0 }
      else if (r.st == 2) { ...r, seen: g, hp: true }
      else { ...r, seen: g, hp: r.hp or r.c != s or r.v != snapV.get(k) }
    })

  // detect_renames: missing files in path order, each takes the first unused new file whose listed size
  // equals the old file's (a content here, sizes differ per content) and whose hash, taken NOW from the
  // folder, equals the old file's: a file written or removed after the listing is judged by what it is now
  pure def pairing(old: int -> Row, snap: int -> int, cur: int -> int, created: Set[int], missing: Set[int]): { used: Set[int], pairs: int -> int } =
    KEYS.fold({ used: Set(), pairs: Map() }, (acc, m) =>
      if (missing.contains(m) and old.get(m).c != 0) {
        val cands = created.filter(n => not(acc.used.contains(n)) and snap.get(n) == old.get(m).c and cur.get(n) == old.get(m).c)
        if (cands.size() > 0) {
          val n = cands.fold(K, (a, x) => if (x < a) x else a)
          { used: acc.used.union(Set(n)), pairs: acc.pairs.put(m, n) }
        } else acc
      } else acc)

  action init = all {
    src' = KEYS.mapBy(k => 0), ver' = KEYS.mapBy(k => 0),
    rows' = KEYS.mapBy(k => NOROW),
    gen' = 0, maxg' = 0,
    ls' = { active: false, g: 0, snap: KEYS.mapBy(k => 0), snapV: KEYS.mapBy(k => 0), complete: true, confirm: false },
    held' = false, attempted' = false, nW' = 0, nP' = 0, nT' = 0,
    quiet' = true, wSince' = false, lastKind' = "init", shrunk' = false, sweptSeenOk' = true, renameOk' = true,
    prevGen' = 0, preDeleted' = false, recreated' = false,
  }

  // every action leaves the variables it does not mention alone
  action keepGhosts: bool = all {
    sweptSeenOk' = sweptSeenOk, renameOk' = renameOk,
  }

  // ---- the folder changes ----
  action write(k: int, c: int): bool = all {
    nW < MAXW, c >= 1, c <= C,
    src' = src.set(k, c), nW' = nW + 1, ver' = ver.set(k, ver.get(k) + 1),
    rows' = rows, gen' = gen, maxg' = maxg, ls' = ls, held' = held, attempted' = attempted, nP' = nP, nT' = nT,
    quiet' = false, wSince' = true, lastKind' = "write", shrunk' = false, prevGen' = gen,
    preDeleted' = preDeleted,
    recreated' = (recreated or (ls.active and ls.snap.get(k) == 0 and rows.get(k).st == 1 and src.get(k) == 0)),
    keepGhosts,
  }
  action remove(k: int): bool = all {
    nW < MAXW, src.get(k) != 0,
    src' = src.set(k, 0), nW' = nW + 1, ver' = ver,
    rows' = rows, gen' = gen, maxg' = maxg, ls' = ls, held' = held, attempted' = attempted, nP' = nP, nT' = nT,
    quiet' = false, wSince' = true, lastKind' = "write", shrunk' = false, prevGen' = gen,
    preDeleted' = (preDeleted or (rows.get(k).st == 1 and rows.get(k).seen >= gen + 1)),
    recreated' = recreated,
    keepGhosts,
  }
  // mv a b: atomic in the folder (a listing sees the object at a or at b, not both, not neither)
  action move(a: int, b: int): bool = all {
    nW < MAXW, a != b, src.get(a) != 0, src.get(b) == 0,
    src' = src.set(b, src.get(a)).set(a, 0), nW' = nW + 1, ver' = ver.set(b, ver.get(b) + 1),
    rows' = rows, gen' = gen, maxg' = maxg, ls' = ls, held' = held, attempted' = attempted, nP' = nP, nT' = nT,
    quiet' = false, wSince' = true, lastKind' = "write", shrunk' = false, prevGen' = gen,
    preDeleted' = (preDeleted or (rows.get(a).st == 1 and rows.get(a).seen >= gen + 1)),
    recreated' = recreated,
    keepGhosts,
  }

  // ---- a full sync ----
  // partial: the listing fails after `cut` keys. confirm: re-run of a held generation.
  action startSync(partial: bool, cut: int, confirm: bool): bool = {
    val g = gen + 1
    val retry = attempted
    all {
      not(ls.active),
      held implies confirm, confirm implies held,
      partial implies (nP < MAXP and cut >= 0 and cut < K),
      ls' = { active: true, g: g, snap: if (partial) KEYS.mapBy(k => if (k < cut) src.get(k) else 0) else src,
              snapV: ver, complete: not(partial), confirm: confirm },
      nP' = if (partial) nP + 1 else nP,
      maxg' = mx(maxg, g), attempted' = true,
      // fix FM3-001: a retried generation forgets the marks of the earlier attempt
      rows' = if (RETRY_RESETS_MARKS and retry)
                KEYS.mapBy(k => if (rows.get(k).st == 1 and rows.get(k).seen >= g) { ...rows.get(k), seen: g - 1 } else rows.get(k))
              else rows,
      src' = src, gen' = gen, held' = held, nW' = nW, nT' = nT, ver' = ver,
      quiet' = quiet, wSince' = false, lastKind' = "start", shrunk' = false, prevGen' = gen,
      preDeleted' = false, recreated' = recreated,
      keepGhosts,
    }
  }

  action finishSync: bool = {
    val g = ls.g
    val rows1 = upsert(rows, ls.snap, ls.snapV, g)
    val created = KEYS.filter(k => rows.get(k).st == 0 and ls.snap.get(k) != 0)
    val missing = KEYS.filter(k => rows1.get(k).st == 1 and rows1.get(k).seen < g)
    val nm = missing.size()
    val liveBefore = countLive(rows)
    val mass = (nm > MASSMIN and nm * 2 > liveBefore) or (allZero(ls.snap) and liveBefore > 0 and nm > 0)
    val pr = pairing(rows, ls.snap, src, created, missing)
    val swept = KEYS.mapBy(k =>
      // delete_file cancels the pending jobs of the file
      if (missing.contains(k)) { ...rows1.get(k), st: 2, hp: false }
      else if (pr.used.contains(k)) { st: 1, seen: g, c: src.get(k), hp: false, v: ver.get(k) }
      else rows1.get(k))
    all {
      ls.active,
      ls' = { ...ls, active: false },
      src' = src, maxg' = maxg, nW' = nW, nP' = nP, nT' = nT, wSince' = wSince, prevGen' = gen, ver' = ver,
      preDeleted' = preDeleted, recreated' = recreated,
      if (not(ls.complete)) all {
        // partial or failed listing: what was seen is recorded, nothing is swept, the generation stays;
        // the attempt overwrites the drive_sync status, so a held generation is no longer held
        rows' = rows1, gen' = gen, held' = false, attempted' = attempted,
        quiet' = quiet, lastKind' = "partial", shrunk' = false, sweptSeenOk' = sweptSeenOk, renameOk' = renameOk,
      } else if (mass and not(ls.confirm)) all {
        // mass delete: held, nothing is swept
        rows' = rows1, gen' = gen, held' = true, attempted' = true,
        quiet' = false, lastKind' = "held", shrunk' = false, sweptSeenOk' = sweptSeenOk, renameOk' = renameOk,
      } else all {
        rows' = swept, gen' = g, held' = false, attempted' = false,
        quiet' = not(wSince), lastKind' = "sweep",
        shrunk' = KEYS.exists(k => rows.get(k).st == 1 and swept.get(k).st != 1),
        sweptSeenOk' = missing.forall(k => rows1.get(k).seen < g),
        renameOk' = pr.pairs.keys().forall(m =>
          created.contains(pr.pairs.get(m)) and src.get(pr.pairs.get(m)) == rows.get(m).c and ls.snap.get(m) == 0),
      }
    }
  }

  // ---- targeted sync (D47) and watcher hint (D46): create or update, never sweep, no generation ----
  action targeted(t: int): bool = all {
    nT < MAXT,
    rows' = if (src.get(t) == 0) rows
            else if (rows.get(t).st == 0) rows.set(t, { st: 1, seen: maxg, c: 0, hp: true, v: 0 })
            else rows.set(t, { ...rows.get(t), seen: maxg, hp: rows.get(t).hp or rows.get(t).st == 2 or rows.get(t).c != src.get(t) or rows.get(t).v != ver.get(t) }),
    nT' = nT + 1, ver' = ver,
    src' = src, gen' = gen, maxg' = maxg, ls' = ls, held' = held, attempted' = attempted, nW' = nW, nP' = nP,
    quiet' = quiet, wSince' = wSince, lastKind' = "targeted", shrunk' = false, prevGen' = gen,
    preDeleted' = preDeleted, recreated' = recreated,
    keepGhosts,
  }

  // hash jobs run: current content is recorded, a deleted row whose file is back is revived; a job
  // whose object is gone finds nothing to do (hashjob.rs nothing_to_do) and leaves the row as it is
  action hashAll: bool = all {
    KEYS.exists(k => rows.get(k).hp),
    rows' = KEYS.mapBy(k => if (rows.get(k).hp and src.get(k) != 0)
                              { st: 1, seen: rows.get(k).seen, c: src.get(k), hp: false, v: ver.get(k) }
                            else if (rows.get(k).hp) { ...rows.get(k), hp: false }
                            else rows.get(k)),
    ver' = ver,
    src' = src, gen' = gen, maxg' = maxg, ls' = ls, held' = held, attempted' = attempted, nW' = nW, nP' = nP, nT' = nT,
    quiet' = quiet, wSince' = wSince, lastKind' = "hash", shrunk' = false, prevGen' = gen,
    preDeleted' = preDeleted, recreated' = recreated,
    keepGhosts,
  }

  action step = any {
    { nondet k = KEYS.oneOf(); nondet c = 1.to(C).oneOf(); write(k, c) },
    { nondet k = KEYS.oneOf(); remove(k) },
    { nondet a = KEYS.oneOf(); nondet b = KEYS.oneOf(); move(a, b) },
    { nondet p = Set(true, false).oneOf(); nondet cut = 0.to(K - 1).oneOf(); nondet cf = Set(true, false).oneOf(); startSync(p, cut, cf) },
    finishSync,
    { nondet t = KEYS.oneOf(); targeted(t) },
    hashAll,
  }

  // ---- invariants ----
  // a live row stops being live only in the sweep of a complete, unheld listing (partial, held, targeted, write, hash never delete)
  val invOnlySweepKills = shrunk implies lastKind == "sweep"
  // the sweep takes only rows that no listing of this generation saw; a targeted touch protects a row
  val invSweepOnlyUnseen = sweptSeenOk
  val invGenerationMonotone = (gen == prevGen or gen == prevGen + 1) and gen <= maxg
  // held only for a generation that has not been committed, and a held sync changed no live row (see invOnlySweepKills)
  val invHeldAfterMassOnly = held implies (gen < maxg and attempted)
  // a rename pairs an old row nobody saw this generation with a new row of the same content
  val invRenameCarries = renameOk
  // a quiet folder: after a complete unheld sync that saw the folder as it is, once the hash jobs are done,
  // the live rows are exactly the objects of the folder with their content
  val invConverged = (quiet and not(KEYS.exists(k => rows.get(k).hp))) implies
    KEYS.forall(k => if (src.get(k) != 0) (rows.get(k).st == 1 and rows.get(k).c == src.get(k)) else rows.get(k).st != 1)
  val inv = invOnlySweepKills and invSweepOnlyUnseen and invGenerationMonotone and invHeldAfterMassOnly and invRenameCarries and invConverged

  val witRename = lastKind == "sweep" and KEYS.exists(k => rows.get(k).st == 2 and rows.get(k).c != 0 and KEYS.exists(n => n != k and rows.get(n).st == 1 and rows.get(n).c == rows.get(k).c))
  val witHeld = held
  val witConfirmedSweep = lastKind == "sweep" and ls.confirm
  val witPartialKeepsDeleted = lastKind == "partial" and KEYS.exists(k => src.get(k) == 0 and rows.get(k).st == 1)
  val witTargetedCreated = lastKind == "targeted" and KEYS.exists(k => rows.get(k).st == 1 and rows.get(k).seen >= gen + 1 and rows.get(k).hp)
  val witRecreatedDuringListing = recreated
  val witRetrySameGeneration = attempted and not(ls.active) and lastKind == "partial"
}
