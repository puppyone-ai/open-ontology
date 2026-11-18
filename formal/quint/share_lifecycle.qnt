// formal: main=share_lifecycle inv=inv steps=40 verify-steps=5 deep-steps=6 witnesses="witDrifted witRefreshed witGroupRefresh witGroupRefreshRolledBack witAtCheckpoint witBlockCheck20 witWarnCheck10 witExpired witRevoked witExtended witCredRejected witLateJoinBackfill witUnreachable witPinnedRefused witGcCollected witMountRemoved" vinv="invCredentialDead invRefreshAtomic invPackBytesImmutable invUpdatesAppendOnly invDriftMarked invChangeDelivered invCheckCodes invCheckFailClosed invGroupSameVersion invGroupAllOrNothing invRefreshPolicy invRetention invExtendKeepsContent invTerminalAbsorbing"
// FM6: Share lifecycle, drift, UPDATES.md, check, task groups, refresh policy and version retention
// (00 5.4, 5.6; 10 7.6-7.8, 16.3-16.6, 16.9; D50, D55, D56, D58, D60, D62). Implementation: crates/oo-docs/src/share/.
// A share is active -> drifted (a used fact was superseded, retracted, or hit by a new constraint violation)
// -> refreshed (terminal, its credential dies, ONE new active share with `src` = old id and the latest
// version appears), or expired / revoked (terminal). The pack bytes of a share never change (`digest`,
// `ver`, `uses` are fixed at creation); the only thing that grows is UPDATES.md: an append-only,
// gap-free list of entries (seq 1..n) with level info / warn / block. A publish writes the entries of
// every live share in the same step ("immediately", stronger than the "eventually" of 80 FM6).
// `check` maps the highest unseen level to exit 0 / 10 / 20, a dead share to 30, an unreachable engine
// to 40 (fail closed); under at-checkpoint a check that comes out 0 or 10 refreshes first (a group
// member refreshes its whole group). A task group refresh is atomic: every live member gets a successor
// at the same new version, or nothing changes. `pinned` shares are never refreshed. The refs credential
// is valid iff it is not revoked and `now < exp`; revoke and refresh kill it in the same step. A share
// joining a group at an older version gets the entries of the versions it missed (backfill), so a change
// that happened before it joined still reaches it. Retention: a version a live share is pinned to keeps
// its cache.
// Bounds: 4 share slots (2 live shares plus successors), 1 group, 4 versions (3 changes), 4 facts, time 0..5.
// Not modelled: the physical mount (the daemon appending to the file is `daemonAppend`, removing the
// directory `removeMount`), compile details (FM8/FM10), sections (one entry per share per version, at the
// worst level of the change), who may call what (auth), `Share.coverage`.
// Notes and reviews: formal/reviews/FM6.md.
module share_lifecycle {
  pure val NS = 4
  pure val SLOTS: Set[int] = 0.to(NS - 1)
  pure val NF = 4
  pure val FACTS: Set[int] = 0.to(NF - 1)
  pure val MAXV = 4
  pure val MAXUPD = 3
  pure val SEQS: Set[int] = 1.to(MAXUPD)
  pure val VERS: Set[int] = 2.to(MAXV)          // versions that carry a change
  pure val MAXT = 5
  pure val LIFE = 2                    // default lifetime (7 days)
  pure val MAXLIFE = 3                 // longest (30 days), counted from creation
  pure val MANUAL = 0
  pure val ATCKPT = 1
  pure val PINNED = 2
  // change kinds of a publish
  pure val ADD = 1
  pure val SUPERSEDE = 2
  pure val RETRACT = 3
  pure val VIOLATE = 4

  type Sh = {
    st: str,               // absent | active | drifted | refreshed | expired | revoked
    ver: int, pol: int, grp: int, src: int, digest: int, uses: Set[int],
    exp: int, created: int, credRev: bool,
    nupd: int, lvl: int -> int, tov: int -> int, applied: int,
    mount: bool,
  }
  type Chk = { used: bool, sid: int, since: int, unreach: bool, code: int, refreshed: bool }
  type Chg = { k: int, f: int }

  var sh: int -> Sh
  var prev: int -> Sh
  var nsh: int
  var dctr: int
  var V: int
  var clog: int -> Chg           // change published as version w (w >= 2)
  var born: int -> int           // version a fact appeared in (0: not yet)
  var died: int -> int           // version it was superseded or retracted (0: live)
  var gver: int -> int           // group version (0: no group yet), group 0 only
  var gpol: int -> int
  var now: int
  var cache: Set[int]
  var chk: Chk
  var cause: str                 // what refreshed in the last step: none | manual | group | check
  var lastAct: str

  pure val ABS: Sh = { st: "absent", ver: 0, pol: 0, grp: -1, src: -1, digest: 0, uses: Set(), exp: 0, created: 0, credRev: false,
                       nupd: 0, lvl: SEQS.mapBy(q => 0), tov: SEQS.mapBy(q => 0), applied: 0, mount: false }
  pure val NOCHK: Chk = { used: false, sid: 0, since: 0, unreach: false, code: -1, refreshed: false }

  pure def live(s: Sh): bool = s.st == "active" or s.st == "drifted"
  pure def terminal(s: Sh): bool = s.st == "refreshed" or s.st == "expired" or s.st == "revoked"
  // a fact is in the pack of version v iff it appeared by v and was not ended by v
  pure def liveAt(b: int -> int, d: int -> int, f: int, v: int): bool = b.get(f) >= 1 and b.get(f) <= v and (d.get(f) == 0 or d.get(f) > v)
  // refs credential: the stored row (not the share's status) decides, with the clock
  pure def credOk(s: Sh, t: int): bool = s.st != "absent" and not(s.credRev) and t < s.exp

  // level a change has for a share that used `uses`: 0 none, 1 info, 2 warn, 3 block
  pure def levelOf(c: Chg, uses: Set[int]): int =
    if (c.k == ADD) 1
    else if (c.k == SUPERSEDE) (if (uses.contains(c.f)) 2 else 0)
    else if (uses.contains(c.f)) 3 else 0

  pure def maxLvl(m: int -> int, since: int, n: int): int =
    SEQS.filter(q => q > since and q <= n).fold(0, (a, q) => if (m.get(q) > a) m.get(q) else a)

  // exit code of `check` for share s (engine reachable)
  pure def checkCode(s: Sh, since: int, t: int): int =
    if (not(live(s)) or t >= s.exp) 30
    else {
      val l = maxLvl(s.lvl, since, s.nupd)
      if (l == 3) 20 else if (l == 2) 10 else 0
    }

  // entries a share gets for the changes of versions (v0, vn] it did not see: one per affecting version, in order
  pure def affecting(cl: int -> Chg, uses: Set[int], v0: int, vn: int): Set[int] =
    VERS.filter(w => w > v0 and w <= vn and levelOf(cl.get(w), uses) > 0)
  pure def rankOf(ws: Set[int], w: int): int = ws.filter(x => x <= w).size()
  pure def backLvl(cl: int -> Chg, uses: Set[int], v0: int, vn: int): int -> int = {
    val ws = affecting(cl, uses, v0, vn)
    SEQS.mapBy(q => ws.filter(w => rankOf(ws, w) == q).fold(0, (a, w) => levelOf(cl.get(w), uses)))
  }
  pure def backTov(cl: int -> Chg, uses: Set[int], v0: int, vn: int): int -> int = {
    val ws = affecting(cl, uses, v0, vn)
    SEQS.mapBy(q => ws.filter(w => rankOf(ws, w) == q).fold(0, (a, w) => w))
  }
  pure def maxOf(m: int -> int, n: int): int = SEQS.filter(q => q <= n).fold(0, (a, q) => if (m.get(q) > a) m.get(q) else a)

  action init = all {
    sh' = SLOTS.mapBy(i => ABS), prev' = SLOTS.mapBy(i => ABS), nsh' = 0, dctr' = 1, V' = 1,
    clog' = 1.to(MAXV).mapBy(w => { k: 0, f: 0 }),
    born' = FACTS.mapBy(f => if (f <= 1) 1 else 0), died' = FACTS.mapBy(f => 0),
    gver' = Set(0).mapBy(g => 0), gpol' = Set(0).mapBy(g => 0), now' = 0, cache' = Set(1),
    chk' = NOCHK, cause' = "none", lastAct' = "init",
  }

  // ---- mount (POST /shares, `oo context mount [--group]`) ----
  action mount = {
    nondet pol0 = Set(MANUAL, ATCKPT, PINNED).oneOf()
    nondet g0 = Set(-1, 0).oneOf()
    nondet us = FACTS.powerset().oneOf()
    val inheriting = g0 == 0 and gver.get(0) > 0
    val ver = if (inheriting) gver.get(0) else V
    val pol = if (inheriting) gpol.get(0) else pol0
    val okUses = us.filter(f => liveAt(born, died, f, ver))
    all {
      nsh < NS,
      okUses.size() > 0,
      // the compile reads the pack at `ver`; versions after it are missed and backfilled
      sh' = sh.set(nsh, { ...ABS, st: if (affecting(clog, okUses, ver, V).filter(w => levelOf(clog.get(w), okUses) >= 2).size() > 0) "drifted" else "active",
                          ver: ver, pol: pol, grp: g0, src: -1, digest: dctr, uses: okUses, exp: now + LIFE, created: now,
                          nupd: affecting(clog, okUses, ver, V).size(), lvl: backLvl(clog, okUses, ver, V), tov: backTov(clog, okUses, ver, V),
                          mount: true }),
      nsh' = nsh + 1, dctr' = dctr + 1,
      gver' = if (g0 == 0 and gver.get(0) == 0) gver.set(0, V) else gver,
      gpol' = if (g0 == 0 and gver.get(0) == 0) gpol.set(0, pol0) else gpol,
      prev' = sh, V' = V, clog' = clog, born' = born, died' = died, now' = now, cache' = cache.union(Set(ver)),
      chk' = NOCHK, cause' = "none", lastAct' = if (inheriting and ver < V) "mountLate" else "mount",
    }
  }

  // ---- an ontology version is published: the change, and the drift of every live share ----
  action publish = {
    nondet k0 = Set(ADD, SUPERSEDE, RETRACT, VIOLATE).oneOf()
    publishK(k0)
  }
  action publishK(k0: int): bool = {
    nondet f0 = FACTS.oneOf()
    val c: Chg = { k: k0, f: f0 }
    val w = V + 1
    val isAdd = k0 == ADD
    all {
      V < MAXV,
      if (isAdd) born.get(f0) == 0 else (liveAt(born, died, f0, V)),
      clog' = clog.set(w, c),
      born' = if (isAdd) born.set(f0, w) else born,
      died' = if (k0 == SUPERSEDE or k0 == RETRACT) died.set(f0, w) else died,
      V' = w,
      sh' = SLOTS.mapBy(i => {
        val s = sh.get(i)
        val l = levelOf(c, s.uses)
        // a share past its end date (not swept yet) is skipped, as `drift::apply` does (`expires_at > now`)
        if (live(s) and now < s.exp and l > 0 and s.nupd < MAXUPD)
          { ...s, nupd: s.nupd + 1, lvl: s.lvl.set(s.nupd + 1, l), tov: s.tov.set(s.nupd + 1, w), st: if (l >= 2) "drifted" else s.st }
        else s
      }),
      cache' = cache.union(Set(w)),
      prev' = sh, nsh' = nsh, dctr' = dctr, gver' = gver, gpol' = gpol, now' = now, chk' = NOCHK, cause' = "none", lastAct' = "publish",
    }
  }

  // refresh the shares in `ms` (single or whole group): each gets one successor at the latest version
  pure def doRefresh(shm: int -> Sh, ms: Set[int], n0: int, v: int, d0: int, t: int, b: int -> int, d: int -> int, gv: int): int -> Sh = {
    val latestUses = FACTS.filter(f => liveAt(b, d, f, v))
    SLOTS.mapBy(i => {
      val s = shm.get(i)
      if (ms.contains(i)) { ...s, st: "refreshed", credRev: true, mount: false }
      else if (i >= n0 and i < n0 + ms.size()) {
        val oldIdx = ms.filter(m => ms.filter(x => x < m).size() == i - n0).fold(0, (a, m) => m)
        val o = shm.get(oldIdx)
        { ...ABS, st: "active", ver: v, pol: o.pol, grp: o.grp, src: oldIdx, digest: d0 + (i - n0), uses: latestUses,
          exp: t + LIFE, created: t, mount: o.mount }
      } else s
    })
  }

  action refresh = {
    nondet s = SLOTS.oneOf()
    val o = sh.get(s)
    all {
      live(o), now < o.exp,                 // an ended share cannot be refreshed
      o.grp == -1,                          // 409 DOCS_SHARE_GROUP_MEMBER
      o.pol != PINNED,                      // 409 DOCS_SHARE_PINNED
      nsh < NS,
      sh' = doRefresh(sh, Set(s), nsh, V, dctr, now, born, died, 0),
      nsh' = nsh + 1, dctr' = dctr + 1, prev' = sh, V' = V, clog' = clog, born' = born, died' = died, gver' = gver, gpol' = gpol,
      now' = now, cache' = cache.union(Set(V)), chk' = NOCHK, cause' = "manual", lastAct' = "refresh",
    }
  }

  // POST /share-groups/{gid}:refresh : all live members or none (a failure rolls everything back)
  action groupRefresh = {
    nondet ok = Set(true, false).oneOf()
    val ms = SLOTS.filter(i => live(sh.get(i)) and now < sh.get(i).exp and sh.get(i).grp == 0)
    all {
      ms.size() > 0,
      gpol.get(0) != PINNED,
      nsh + ms.size() <= NS,
      if (ok) all {
        sh' = doRefresh(sh, ms, nsh, V, dctr, now, born, died, 0),
        nsh' = nsh + ms.size(), dctr' = dctr + ms.size(), gver' = gver.set(0, V), cause' = "group",
      } else all {
        sh' = sh, nsh' = nsh, dctr' = dctr, gver' = gver, cause' = "none",
      },
      prev' = sh, V' = V, clog' = clog, born' = born, died' = died, gpol' = gpol, now' = now, cache' = if (ok) cache.union(Set(V)) else cache, chk' = NOCHK,
      lastAct' = if (ok) "groupRefresh" else "groupRefreshRolledBack",
    }
  }

  // ---- oo context check [--since n]; the engine is reachable or not ----
  action check = {
    nondet s = SLOTS.oneOf()
    nondet since0 = 0.to(MAXUPD).oneOf()
    nondet unr = Set(true, false).oneOf()
    val o = sh.get(s)
    val code = if (unr) 40 else checkCode(o, since0, now)
    // at-checkpoint: a check that comes out 0 or 10 first refreshes (the group for a member)
    val auto = not(unr) and (code == 0 or code == 10) and o.pol == ATCKPT and o.grp == -1 and
               nsh < NS and o.ver < V
    val gms = SLOTS.filter(i => live(sh.get(i)) and now < sh.get(i).exp and sh.get(i).grp == 0)
    val autoG = not(unr) and (code == 0 or code == 10) and o.pol == ATCKPT and o.grp == 0 and gms.size() > 0 and
                nsh + gms.size() <= NS and o.ver < V
    all {
      o.st != "absent",
      chk' = { used: true, sid: s, since: since0, unreach: unr, code: code, refreshed: auto or autoG },
      if (auto) all {
        sh' = doRefresh(sh, Set(s), nsh, V, dctr, now, born, died, 0), nsh' = nsh + 1, dctr' = dctr + 1, gver' = gver, cause' = "check",
      } else if (autoG) all {
        sh' = doRefresh(sh, gms, nsh, V, dctr, now, born, died, 0), nsh' = nsh + gms.size(), dctr' = dctr + gms.size(),
        gver' = gver.set(0, V), cause' = "check",
      } else all {
        sh' = sh, nsh' = nsh, dctr' = dctr, gver' = gver, cause' = "none",
      },
      prev' = sh, V' = V, clog' = clog, born' = born, died' = died, gpol' = gpol, now' = now,
      cache' = if (auto or autoG) cache.union(Set(V)) else cache, lastAct' = "check",
    }
  }

  // ---- revoke, expiry, extension ----
  action revoke = {
    nondet s = SLOTS.oneOf()
    all {
      live(sh.get(s)),
      sh' = sh.set(s, { ...sh.get(s), st: "revoked", credRev: true }),
      prev' = sh, nsh' = nsh, dctr' = dctr, V' = V, clog' = clog, born' = born, died' = died, gver' = gver, gpol' = gpol, now' = now, cache' = cache,
      chk' = NOCHK, cause' = "none", lastAct' = "revoke",
    }
  }

  action tick = all {
    now < MAXT,
    now' = now + 1, sh' = sh, prev' = sh, nsh' = nsh, dctr' = dctr, V' = V, clog' = clog, born' = born, died' = died,
    gver' = gver, gpol' = gpol, cache' = cache, chk' = NOCHK, cause' = "none", lastAct' = "tick",
  }

  // the scheduler's sweep: expires_at <= now
  action expireSweep = {
    val due = SLOTS.filter(i => live(sh.get(i)) and sh.get(i).exp <= now)
    all {
      due.size() > 0,
      sh' = SLOTS.mapBy(i => if (due.contains(i)) { ...sh.get(i), st: "expired" } else sh.get(i)),
      prev' = sh, nsh' = nsh, dctr' = dctr, V' = V, clog' = clog, born' = born, died' = died, gver' = gver, gpol' = gpol, now' = now, cache' = cache,
      chk' = NOCHK, cause' = "none", lastAct' = "expireSweep",
    }
  }

  // PATCH expires_at: only the end moves; content and version stay
  action extend = {
    nondet s = SLOTS.oneOf()
    nondet e = 1.to(MAXLIFE + MAXT).oneOf()
    val o = sh.get(s)
    all {
      live(o), now < o.exp,
      e > o.exp, e <= o.created + MAXLIFE,
      sh' = sh.set(s, { ...o, exp: e }),
      prev' = sh, nsh' = nsh, dctr' = dctr, V' = V, clog' = clog, born' = born, died' = died, gver' = gver, gpol' = gpol, now' = now, cache' = cache,
      chk' = NOCHK, cause' = "none", lastAct' = "extend",
    }
  }

  // ---- the daemon ----
  // appends the next UPDATES.md entry to the mounted directory (only while the share is live)
  action daemonAppend = {
    nondet s = SLOTS.oneOf()
    all {
      sh.get(s).mount, sh.get(s).applied < sh.get(s).nupd,
      sh' = sh.set(s, { ...sh.get(s), applied: sh.get(s).applied + 1 }),
      prev' = sh, nsh' = nsh, dctr' = dctr, V' = V, clog' = clog, born' = born, died' = died, gver' = gver, gpol' = gpol, now' = now, cache' = cache,
      chk' = NOCHK, cause' = "none", lastAct' = "daemonAppend",
    }
  }
  // deletes the directory of a share that ended (revoked / expired; a refreshed one is replaced)
  action removeMount = {
    nondet s = SLOTS.oneOf()
    all {
      sh.get(s).mount, sh.get(s).st == "revoked" or sh.get(s).st == "expired",
      sh' = sh.set(s, { ...sh.get(s), mount: false }),
      prev' = sh, nsh' = nsh, dctr' = dctr, V' = V, clog' = clog, born' = born, died' = died, gver' = gver, gpol' = gpol, now' = now, cache' = cache,
      chk' = NOCHK, cause' = "none", lastAct' = "removeMount",
    }
  }

  // a refs read with the credential of a share
  action refsRead = {
    nondet s = SLOTS.oneOf()
    all {
      sh.get(s).st != "absent",
      chk' = { used: true, sid: s, since: 0, unreach: false, code: if (credOk(sh.get(s), now)) 1 else 0, refreshed: false },
      sh' = sh, prev' = sh, nsh' = nsh, dctr' = dctr, V' = V, clog' = clog, born' = born, died' = died, gver' = gver, gpol' = gpol, now' = now, cache' = cache,
      cause' = "none", lastAct' = "refsRead",
    }
  }

  // retention (D58): the cache of a version nobody is pinned to may go (30 days are `aged`)
  pure def pinnedBy(shm: int -> Sh, v: int): int = SLOTS.filter(i => live(shm.get(i)) and shm.get(i).ver == v).size()
  action gc = {
    nondet v = 1.to(MAXV).oneOf()
    all {
      cache.contains(v), pinnedBy(sh, v) == 0,
      cache' = cache.exclude(Set(v)),
      sh' = sh, prev' = sh, nsh' = nsh, dctr' = dctr, V' = V, clog' = clog, born' = born, died' = died, gver' = gver, gpol' = gpol, now' = now,
      chk' = NOCHK, cause' = "none", lastAct' = "gc",
    }
  }

  // the profile `conformance_share.rs` replays on the live engine: publishes only add facts (a share's `uses` do not matter then:
  // every share in scope gets an info entry), no extension (PATCH /shares is not implemented), no daemon or gc steps
  action stepReplay = any { mount, all { publishK(ADD) }, refresh, groupRefresh, check, revoke, tick, expireSweep, refsRead }
  action step = any { mount, publish, refresh, groupRefresh, check, revoke, tick, expireSweep, extend, daemonAppend, removeMount, refsRead, gc }

  // ---- invariants ----
  val ALLSH = SLOTS.filter(i => sh.get(i).st != "absent")
  // refs credential: dead once the share is revoked, refreshed or expired (time or sweep)
  val invCredentialDead = SLOTS.forall(i => (terminal(sh.get(i)) or (live(sh.get(i)) and now >= sh.get(i).exp)) implies not(credOk(sh.get(i), now))) and
                          (lastAct == "refsRead" implies (chk.code == 1) == credOk(sh.get(chk.sid), now))
  // refresh: the old share is refreshed (terminal) and has exactly one successor, which is not older; no orphan successor
  val invRefreshAtomic = SLOTS.forall(i => {
    val s = sh.get(i)
    val succ = SLOTS.filter(j => sh.get(j).st != "absent" and sh.get(j).src == i)
    (s.st == "refreshed" implies (succ.size() == 1 and succ.forall(j => j > i and sh.get(j).ver >= s.ver and sh.get(j).grp == s.grp))) and
    (s.st != "refreshed" implies succ.size() == 0) and
    (s.st != "absent" and s.src >= 0 implies (s.src < i and sh.get(s.src).st == "refreshed"))
  })
  // the pack of a share never changes: digest, version, used facts, policy, group fixed from creation
  val invPackBytesImmutable = SLOTS.forall(i => prev.get(i).st != "absent" implies
    (sh.get(i).digest == prev.get(i).digest and sh.get(i).ver == prev.get(i).ver and sh.get(i).uses == prev.get(i).uses and
     sh.get(i).pol == prev.get(i).pol and sh.get(i).grp == prev.get(i).grp and sh.get(i).src == prev.get(i).src))
  // UPDATES.md: seq 1..n without gap, levels 1..3, existing entries never change, never fewer, the daemon never ahead
  val invUpdatesAppendOnly = SLOTS.forall(i => {
    val s = sh.get(i)
    val p = prev.get(i)
    SEQS.forall(q => (q <= s.nupd) == (s.lvl.get(q) > 0)) and SEQS.forall(q => q <= s.nupd implies (s.lvl.get(q) <= 3 and s.tov.get(q) >= 2)) and
    s.nupd >= p.nupd and SEQS.forall(q => q <= p.nupd implies (s.lvl.get(q) == p.lvl.get(q) and s.tov.get(q) == p.tov.get(q))) and
    s.applied <= s.nupd and s.applied >= p.applied and
    SEQS.forall(q => (q > 1 and q <= s.nupd) implies s.tov.get(q) > s.tov.get(q - 1))
  })
  // D50: a live share that used a fact that was superseded, retracted or hit by a violation after its version is drifted
  val invDriftMarked = SLOTS.forall(i => {
    val s = sh.get(i)
    (live(s) and now < s.exp) implies (VERS.filter(w => w > s.ver and w <= V and levelOf(clog.get(w), s.uses) >= 2).size() > 0 implies s.st == "drifted")
  })
  // change delivery: every such change has an entry of at least its level, at that version
  val invChangeDelivered = SLOTS.forall(i => {
    val s = sh.get(i)
    (live(s) and now < s.exp) implies VERS.forall(w => (w > s.ver and w <= V and levelOf(clog.get(w), s.uses) > 0) implies
      SEQS.exists(q => q <= s.nupd and s.tov.get(q) == w and s.lvl.get(q) >= levelOf(clog.get(w), s.uses)))
  })
  // exit codes: 30 ended or past expiry, 20 a block entry above `since`, 10 a warn, else 0; never 0 or 10 over a block
  val invCheckCodes = (lastAct == "check" and not(chk.unreach)) implies {
    val s = prev.get(chk.sid)       // the share as the check saw it
    val worst = maxLvl(s.lvl, chk.since, s.nupd)
    ((chk.code == 30) == (not(live(s)) or now >= s.exp)) and
    ((live(s) and now < s.exp) implies ((chk.code == 20) == (worst == 3) and (chk.code == 10) == (worst == 2) and (chk.code == 0) == (worst <= 1)))
  }
  // fail closed: no answer is 40, and 0 only comes from a reachable engine
  val invCheckFailClosed = (lastAct == "check") implies ((chk.unreach implies (chk.code == 40 and not(chk.refreshed))) and (chk.code == 0 implies not(chk.unreach)))
  // a task group's live members (not past their end, an unswept expiry is already dead to check and refs) are on one version
  val invGroupSameVersion = SLOTS.forall(i => SLOTS.forall(j =>
    (live(sh.get(i)) and live(sh.get(j)) and now < sh.get(i).exp and now < sh.get(j).exp and sh.get(i).grp == 0 and sh.get(j).grp == 0) implies sh.get(i).ver == sh.get(j).ver))
  // a group refresh moves every live member or none; a rolled back one changes nothing
  val invGroupAllOrNothing = (lastAct == "groupRefresh" or lastAct == "groupRefreshRolledBack") implies {
    val before = SLOTS.filter(i => live(prev.get(i)) and now < prev.get(i).exp and prev.get(i).grp == 0)
    val moved = before.filter(i => sh.get(i).st == "refreshed")
    (moved == before or moved == Set()) and
    (lastAct == "groupRefreshRolledBack" implies (sh == prev)) and
    (lastAct == "groupRefresh" implies (moved == before and before.size() > 0))
  }
  // refresh policy: pinned is never refreshed; manual only by an explicit refresh; at-checkpoint also by a check that gave 0 or 10
  val invRefreshPolicy = SLOTS.forall(i => sh.get(i).st == "refreshed" implies sh.get(i).pol != PINNED) and
    SLOTS.forall(i => (prev.get(i).st != "refreshed" and sh.get(i).st == "refreshed") implies
      (cause == "manual" or cause == "group" or
       (cause == "check" and sh.get(i).pol == ATCKPT and (chk.code == 0 or chk.code == 10) and chk.refreshed)))
  // a version a live share is pinned to keeps its cache (D58)
  val invRetention = SLOTS.forall(i => live(sh.get(i)) implies cache.contains(sh.get(i).ver))
  // extending changes the end date only
  val invExtendKeepsContent = lastAct == "extend" implies SLOTS.forall(i =>
    sh.get(i).st == prev.get(i).st and sh.get(i).nupd == prev.get(i).nupd and sh.get(i).ver == prev.get(i).ver and
    sh.get(i).exp >= prev.get(i).exp and sh.get(i).exp <= sh.get(i).created + MAXLIFE)
  // refreshed, expired and revoked never come back
  val invTerminalAbsorbing = SLOTS.forall(i => terminal(prev.get(i)) implies sh.get(i).st == prev.get(i).st)

  val inv = invCredentialDead and invRefreshAtomic and invPackBytesImmutable and invUpdatesAppendOnly and invDriftMarked and
            invChangeDelivered and invCheckCodes and invCheckFailClosed and invGroupSameVersion and invGroupAllOrNothing and
            invRefreshPolicy and invRetention and invExtendKeepsContent and invTerminalAbsorbing

  // ---- witnesses ----
  val witDrifted = SLOTS.exists(i => sh.get(i).st == "drifted")
  val witRefreshed = SLOTS.exists(i => sh.get(i).st == "refreshed")
  val witGroupRefresh = lastAct == "groupRefresh"
  val witGroupRefreshRolledBack = lastAct == "groupRefreshRolledBack"
  val witAtCheckpoint = lastAct == "check" and chk.refreshed
  val witBlockCheck20 = lastAct == "check" and chk.code == 20
  val witWarnCheck10 = lastAct == "check" and chk.code == 10
  val witExpired = SLOTS.exists(i => sh.get(i).st == "expired")
  val witRevoked = SLOTS.exists(i => sh.get(i).st == "revoked")
  val witExtended = lastAct == "extend"
  val witCredRejected = lastAct == "refsRead" and chk.code == 0
  val witLateJoinBackfill = lastAct == "mountLate" and SLOTS.exists(i => sh.get(i).src == -1 and sh.get(i).nupd > 0 and sh.get(i).created == now and sh.get(i).st != "absent" and sh.get(i).ver < V)
  val witUnreachable = lastAct == "check" and chk.unreach
  val witPinnedRefused = SLOTS.exists(i => live(sh.get(i)) and sh.get(i).pol == PINNED and sh.get(i).ver < V)
  val witGcCollected = lastAct == "gc"
  val witMountRemoved = lastAct == "removeMount"
}
