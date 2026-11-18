// formal: main=visibility inv=inv steps=6 verify-steps=2 deep-steps=3 witnesses="witFactBlocked witAliasFiltered witEntityBlocked witEvidenceFiltered witHumanFactFiltered witWithdrawnIgnored witScopeGrows" vinv="invFactsInScope invEvidenceOnlyInScope invEntitiesSupported invAliasesInScope invNoLeakAliases invNoLeakFactEnds invHumanNoBackdoor invBlockedFactCount invScopeMonotone invOutputWithinInput invBlockedFactsNotVisible" dinv="invBlockedEntityCount invBlockedHidden invNoLeakEntities"
// FM8: visibility (D26, 10 7.2 `compile/visible.rs`; Kani KP5 proves the same on the Rust function).
// A fact is visible iff at least one of its live (not withdrawn) evidence rows lies in the share scope;
// only in-scope evidence is attached; an entity or alias appears only if some in-scope live evidence
// supports it; blocked counts equal what the filter dropped (over the whole Drive, as the implementation counts).
// The graph is chosen once at init (4 files, 6 entities, 10 facts, 6 aliases, alias evidence across
// files, withdrawn evidence, human overlay facts) and every step compiles it against a new scope.
// The compile is written the way `VisibleGraph::load` + the BRIEF expansion do it (scope the evidence
// rows first, derive visible facts, then entities and aliases from those, then expand one hop);
// the invariants restate D26 from the unfiltered graph, so a filter that lets anything through fails.
// Not modelled: text rendering and the leak-guard word scan (compile/leakguard.rs, covered by the
// red-team gate), token budgets, file versions (a file stands for the version in the snapshot).
// Notes and the two reviews: formal/reviews/FM8.md.
module visibility {
  pure val FILES: Set[int] = 0.to(3)
  pure val ENT: Set[int] = 0.to(5)
  pure val FACTS: Set[int] = 0.to(9)
  pure val ALIASES: Set[int] = 0.to(5)
  pure val NONE = -1

  type Graph = {
    subj: int -> int,
    obj: int -> int,            // NONE for attribute facts
    ev: int -> Set[int],        // files holding an evidence row of the fact
    wd: int -> Set[int],        // files whose evidence row is withdrawn
    human: int -> bool,         // origin = human (overlay): no back door past D26 (10 7.2 item 5)
    aowner: int -> int,
    aev: int -> Set[int],
    awd: int -> Set[int],
  }
  type Out = {
    facts: Set[int],
    ev: int -> Set[int],
    ents: Set[int],
    aliases: Set[int],
    blockedF: int,
    blockedE: int,
  }

  var g: Graph
  var seeds: Set[int]
  var scope: Set[int]
  var prevScope: Set[int]
  var prevFacts: Set[int]
  var out: Out

  // ---- the compile, shaped like the implementation ----
  pure def liveEv(gr: Graph, f: int): Set[int] = gr.ev.get(f).exclude(gr.wd.get(f))
  pure def scopedEv(gr: Graph, sc: Set[int], f: int): Set[int] = liveEv(gr, f).intersect(sc)
  pure def aliasScoped(gr: Graph, sc: Set[int], a: int): Set[int] = gr.aev.get(a).exclude(gr.awd.get(a)).intersect(sc)
  pure def visFacts(gr: Graph, sc: Set[int]): Set[int] = FACTS.filter(f => scopedEv(gr, sc, f).size() > 0)
  pure def visAliases(gr: Graph, sc: Set[int]): Set[int] = ALIASES.filter(a => aliasScoped(gr, sc, a).size() > 0)
  pure def ends(gr: Graph, f: int): Set[int] =
    if (gr.obj.get(f) == NONE) Set(gr.subj.get(f)) else Set(gr.subj.get(f), gr.obj.get(f))
  pure def unionEnds(gr: Graph, fs: Set[int]): Set[int] = fs.fold(Set(), (acc, f) => acc.union(ends(gr, f)))
  pure def visEntities(gr: Graph, sc: Set[int]): Set[int] = {
    val vf = visFacts(gr, sc)
    val va = visAliases(gr, sc)
    ENT.filter(e => vf.exists(f => ends(gr, f).contains(e)) or va.exists(a => gr.aowner.get(a) == e))
  }

  // what the filter dropped: facts that have a live evidence row but none in scope, and the entities
  // only they (or an out-of-scope alias) point at. The implementation counts them over the whole
  // Drive (`MemoryReader::scoped`, `pg.rs`), not only one hop from the seeds as 10 7.2 words it.
  pure def blockedFacts(gr: Graph, sc: Set[int]): Set[int] =
    FACTS.filter(f => liveEv(gr, f).size() > 0 and scopedEv(gr, sc, f).size() == 0)
  pure def blockedEntities(gr: Graph, sc: Set[int]): Set[int] = {
    val viaFacts = unionEnds(gr, blockedFacts(gr, sc))
    val viaAliases = ALIASES.filter(a => gr.aev.get(a).exclude(gr.awd.get(a)).size() > 0 and aliasScoped(gr, sc, a).size() == 0).map(a => gr.aowner.get(a))
    viaFacts.union(viaAliases).exclude(visEntities(gr, sc))
  }

  pure def compile(gr: Graph, sc: Set[int], sd: Set[int]): Out = {
    val ve = visEntities(gr, sc)
    val vf = visFacts(gr, sc)
    val sv = sd.intersect(ve)                       // an invisible seed is an error, not a count (7.3)
    val touch = FACTS.filter(f => liveEv(gr, f).size() > 0 and ends(gr, f).exists(e => sv.contains(e)))
    val hop1 = touch.intersect(vf)
    val ents = sv.union(unionEnds(gr, hop1))
    val als = visAliases(gr, sc).filter(a => ents.contains(gr.aowner.get(a)))
    {
      facts: hop1,
      ev: FACTS.mapBy(f => if (hop1.contains(f)) scopedEv(gr, sc, f) else Set()),
      ents: ents,
      aliases: als,
      blockedF: blockedFacts(gr, sc).size(),
      blockedE: blockedEntities(gr, sc).size(),
    }
  }

  action init = {
    nondet s = FACTS.setOfMaps(ENT).oneOf()
    nondet o = FACTS.setOfMaps(ENT.union(Set(NONE))).oneOf()
    nondet e = FACTS.setOfMaps(FILES.powerset()).oneOf()
    nondet w = FACTS.setOfMaps(FILES.powerset()).oneOf()
    nondet h = FACTS.setOfMaps(Set(true, false)).oneOf()
    nondet ao = ALIASES.setOfMaps(ENT).oneOf()
    nondet ae = ALIASES.setOfMaps(FILES.powerset()).oneOf()
    nondet aw = ALIASES.setOfMaps(FILES.powerset()).oneOf()
    nondet sd = ENT.powerset().oneOf()
    nondet sc = FILES.powerset().oneOf()
    val gr: Graph = { subj: s, obj: o, ev: e, wd: w, human: h, aowner: ao, aev: ae, awd: aw }
    all {
      g' = gr,
      seeds' = sd,
      scope' = sc,
      prevScope' = Set(),
      prevFacts' = Set(),
      out' = compile(gr, sc, sd),
    }
  }

  // a share is compiled again with another scope (edited scope, refresh with the same params)
  action step = {
    nondet sc = FILES.powerset().oneOf()
    all {
      g' = g,
      seeds' = seeds,
      scope' = sc,
      prevScope' = scope,
      prevFacts' = out.facts,
      out' = compile(g, sc, seeds),
    }
  }

  // ---- invariants: D26 restated from the unfiltered graph ----
  // INDEPENDENT of the compile helpers (liveEv, scopedEv, aliasScoped, ends): spelled out on the raw maps, so a
  // mutation of a helper cannot change both the compile and its invariants (review B1).
  pure def evInScope(gr: Graph, sc: Set[int], f: int): bool =
    gr.ev.get(f).exists(x => sc.contains(x) and not(gr.wd.get(f).contains(x)))
  pure def aliasEvInScope(gr: Graph, sc: Set[int], a: int): bool =
    gr.aev.get(a).exists(x => sc.contains(x) and not(gr.awd.get(a).contains(x)))
  pure def supported(gr: Graph, sc: Set[int], e: int): bool =
    FACTS.exists(f => evInScope(gr, sc, f) and (gr.subj.get(f) == e or gr.obj.get(f) == e)) or
    ALIASES.exists(a => gr.aowner.get(a) == e and aliasEvInScope(gr, sc, a))

  // every fact of the pack has at least one in-scope live evidence row
  val invFactsInScope = out.facts.forall(f => evInScope(g, scope, f))
  // and carries exactly its in-scope live evidence, nothing else
  val invEvidenceOnlyInScope = out.facts.forall(f =>
    out.ev.get(f).size() > 0 and out.ev.get(f).subseteq(scope) and out.ev.get(f).subseteq(g.ev.get(f)) and
    out.ev.get(f).intersect(g.wd.get(f)) == Set())
  // an entity is in the pack only through in-scope evidence
  val invEntitiesSupported = out.ents.forall(e => supported(g, scope, e))
  // an alias is in the pack only through its own in-scope evidence, on a pack entity
  val invAliasesInScope = out.aliases.forall(a => aliasEvInScope(g, scope, a) and out.ents.contains(g.aowner.get(a)))
  // names that exist only out of scope appear nowhere: no entity, alias, or fact end
  pure def hiddenEnts(gr: Graph, sc: Set[int]): Set[int] = ENT.filter(e => not(supported(gr, sc, e)))
  // no entity that exists only out of scope is in the pack
  val invNoLeakEntities = out.ents.intersect(hiddenEnts(g, scope)) == Set()
  // no alias of such an entity is in the pack
  val invNoLeakAliases = out.aliases.forall(a => not(hiddenEnts(g, scope).contains(g.aowner.get(a))))
  // no fact of the pack has such an entity at an end
  val invNoLeakFactEnds = out.facts.forall(f => not(hiddenEnts(g, scope).contains(g.subj.get(f))) and not(hiddenEnts(g, scope).contains(g.obj.get(f))))
  // all three (simulation; Apalache checks them one by one, the conjunction exceeds the FAST budget)
  val invNoLeak = invNoLeakEntities and invNoLeakAliases and invNoLeakFactEnds
  // pointwise form of invBlockedHidden's fact part: a fact whose live evidence is all out of scope is neither in the pack nor visible
  val invBlockedFactsNotVisible = FACTS.forall(f =>
    (gr0Of(g, f) and not(evInScope(g, scope, f))) implies (not(out.facts.contains(f)) and not(visFacts(g, scope).contains(f))))
  pure def gr0Of(gr: Graph, f: int): bool = gr.ev.get(f).exists(x => not(gr.wd.get(f).contains(x)))
  // a human overlay fact without in-scope evidence is invisible too
  val invHumanNoBackdoor = FACTS.forall(f => (g.human.get(f) and scopedEv(g, scope, f).size() == 0) implies not(out.facts.contains(f)))
  // blocked counts equal what the filter dropped, restated from the unfiltered graph
  pure def droppedFacts(gr: Graph, sc: Set[int]): Set[int] =
    FACTS.filter(f => gr.ev.get(f).exclude(gr.wd.get(f)).size() > 0 and gr.ev.get(f).exclude(gr.wd.get(f)).intersect(sc) == Set())
  val invBlockedFactCount = out.blockedF == droppedFacts(g, scope).size()
  val invBlockedEntityCount = {
    val viaAlias = ALIASES.filter(a => g.aev.get(a).exclude(g.awd.get(a)).size() > 0).map(a => g.aowner.get(a))
    val hidden = unionEnds(g, droppedFacts(g, scope)).union(viaAlias).filter(e => not(supported(g, scope, e)))
    out.blockedE == hidden.size()
  }
  val invBlockedCounts = invBlockedFactCount and invBlockedEntityCount
  // a blocked fact or entity is counted and nowhere else: not in the pack, not visible at all
  val invBlockedHidden = {
    val dropped = FACTS.filter(f => g.ev.get(f).exclude(g.wd.get(f)).size() > 0 and g.ev.get(f).exclude(g.wd.get(f)).intersect(scope) == Set())
    val hiddenEnts = ENT.filter(e => not(supported(g, scope, e)))
    dropped.intersect(out.facts) == Set() and dropped.intersect(visFacts(g, scope)) == Set() and
    hiddenEnts.intersect(out.ents) == Set() and ALIASES.forall(a => hiddenEnts.contains(g.aowner.get(a)) implies not(out.aliases.contains(a)))
  }
  // widening the scope never removes a fact of the pack
  val invScopeMonotone = prevScope.subseteq(scope) implies prevFacts.subseteq(out.facts)
  // KP5 shape: the output is a subset of the visible facts of the input
  val invOutputWithinInput = out.facts.subseteq(visFacts(g, scope)) and out.ents.subseteq(ENT) and out.aliases.subseteq(ALIASES)

  val inv = invFactsInScope and invEvidenceOnlyInScope and invEntitiesSupported and invAliasesInScope and
            invNoLeak and invHumanNoBackdoor and invBlockedCounts and invBlockedHidden and invScopeMonotone and invOutputWithinInput

  // ---- witnesses (reachability) ----
  val witFactBlocked = out.blockedF > 0
  val witEntityBlocked = out.blockedE > 0
  val witAliasFiltered = out.ents.exists(e => ALIASES.exists(a => g.aowner.get(a) == e and not(out.aliases.contains(a))))
  val witEvidenceFiltered = out.facts.exists(f => g.ev.get(f).size() > out.ev.get(f).size())
  val witHumanFactFiltered = FACTS.exists(f => g.human.get(f) and liveEv(g, f).size() > 0 and scopedEv(g, scope, f).size() == 0)
  val witWithdrawnIgnored = FACTS.exists(f => g.wd.get(f).intersect(g.ev.get(f)).intersect(scope).size() > 0 and scopedEv(g, scope, f).size() == 0)
  val witScopeGrows = prevScope != scope and prevScope.subseteq(scope) and out.facts.size() > prevFacts.size()
}
