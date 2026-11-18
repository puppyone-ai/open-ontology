# FM8 review: visibility (`formal/quint/visibility.qnt`)

Model of D26 (10 section 7.2, `compile/visible.rs`, `compile/kernels.rs::visible_decide`). Spec: 00 D26, 80 section 3.1 (FM8) and
section 3.2 (KP5). Card V-S5-02.

Review status: **two reviewers are required** (80 section 7, card V-S5-02: this is the safety promise). This document is
reviewer 1 (the author, lane A, 2026-11-17). Reviewer 2 (independent, run by the board lead) is **not yet done**; the section
"Where to attack" lists what an independent reviewer should try to break.

## 1. Model shape

- One static graph chosen at init: 4 files, 6 entities, 10 facts (subject, object or none, evidence = the files holding an
  evidence row, withdrawn evidence, `human` overlay origin), 6 aliases (owner, evidence files, withdrawn). The share scope is a set
  of files (a file stands for the file version of the snapshot); seeds are a set of entities. Every step compiles the same graph
  against a new scope (`scope`, `prevScope`, `prevFacts` carry the previous compile).
- `compile` is written the way the code does it: scope the evidence rows, derive visible facts, derive entities and aliases from
  them, expand one hop from the visible seeds, count what was dropped. The invariants restate D26 from the unfiltered graph.
- Bounds: the graph is fully symbolic (no enumeration of graphs), one compile at init plus any number of recompiles.

## 2. Rule to model to code

| D26 / 7.2 rule | Code | Model | Invariant |
|---|---|---|---|
| a fact is visible iff it has at least one live evidence row in scope | `from_scoped` -> `visible_decide` (`fact_has_scoped_ev`) | `visFacts`, `scopedEv` | `invFactsInScope`, `invOutputWithinInput` |
| only in-scope evidence is attached | `visible_ev` (snapshot membership) | `compile.ev` | `invEvidenceOnlyInScope` |
| entity visible iff a visible fact or a visible alias points at it | `visible_decide` (`entity_visible`) | `visEntities` | `invEntitiesSupported` |
| aliases only through their own in-scope evidence | `alias_visible` | `visAliases` | `invAliasesInScope` |
| names that exist only out of scope appear nowhere (entities, aliases, fact ends) | the three above | `hidden` in `invNoLeak` | `invNoLeak` |
| a human overlay fact has no back door (7.2 item 5) | no special case in `from_scoped` | `human` map | `invHumanNoBackdoor` |
| blocked counts = what the filter dropped | `MemoryReader::scoped`, `pg.rs` (`blocked`) | `blockedFacts`, `blockedEntities` | `invBlockedCounts`, `invBlockedHidden` |
| widening the scope never removes a fact | follows from the above | `prevFacts` | `invScopeMonotone` |

## 3. Readings of the spec the model made explicit

1. **Blocked counts are over the whole Drive, not one hop from the seeds.** 10 section 7.2 item 2 words it as "the facts and
   entities one hop from the seeds that have evidence but none in scope". `MemoryReader::scoped` and `pg.rs` count every fact with a live
   evidence row and none in scope, and every entity only such facts or aliases point at. The model follows the code and the 80 wording
   ("the blocked count equals the number of filtered-out items"); mutation M5 (the 1-hop wording) is caught. Needs the board lead's
   confirmation; the count is shown to the person who shares, so the whole-Drive number is the more informative one, and it is a
   count only.
2. The leak guard (word scan of the output) is **not** modelled: FM8 proves the graph that feeds the renderers; the scan is covered by
   the red-team gate (`gates_live.rs`) and by `conformance_visibility.rs`, which compiles whole packs for the model's graphs and scans
   every output byte for the names that exist only out of scope.

## 4. Mutation checks (simulation, 30,000 traces)

| # | Mutation | Caught by |
|---|---|---|
| M1 | a human overlay fact is visible without evidence in scope | `invHumanNoBackdoor`, `invFactsInScope` |
| M2 | withdrawn evidence still counts | `invEvidenceOnlyInScope` |
| M3 | an alias is visible whenever its owner is | `invAliasesInScope` |
| M4 | the pack attaches all live evidence, not only in-scope | `invEvidenceOnlyInScope` |
| M5 | blocked facts counted one hop from the seeds | `invBlockedFactCount` |
| M6 | alias scope ignored in the helper (reviewer M5) | `invAliasesInScope`, `invNoLeakAliases`, `invNoLeakEntities`, `invEntitiesSupported` |
| M7 | fact scope ignored in the helper | `invFactsInScope`, `invEvidenceOnlyInScope` |
| M8 | alias withdrawal ignored in the helper | `invAliasesInScope` |

After review B1 (2026-11-17) the invariants `invFactsInScope`, `invAliasesInScope` and `supported` (hence `invEntitiesSupported`, `invNoLeak*`) are written on the raw graph maps (`evInScope`, `aliasEvInScope`), not on the compile's helpers, so M6 to M8 fail where before they passed.

## 5. Where to attack (for reviewer 2)

- Evidence is per file version in the real system; the model collapses a file to one version. A path that is in scope in one
  version and out in another (rename) is the SQL `scope_versions` CTE, not this model: look at `pg.rs`.
- The model has no multi-hop expansion; the supersedes chain (+1 hop) and "possibly meant" candidates in BRIEF are not here. They
  only ever read `VisibleGraph`, which is the claim being modelled.
- `refs/` quotes (`VisibleGraph::quote`) read blocks of in-scope versions only; this is `compile/tests.rs` and `gates_live.rs`, not FM8.
- Look for any accessor on `VisibleGraph` that returns something the model's `out` does not have.

## 6. Conformance and proofs

`conformance_visibility.rs` (no database): every model state is rebuilt in a `MemoryReader`, loaded through `VisibleGraph::load`, and the
pack facts, evidence files, entities, aliases and both blocked counts are compared; a whole pack is compiled and scanned.
KP5 (`compile/proofs.rs`, `kernels.rs::visible_decide`): the same statements proved by Kani on the function the compiler calls.

## Independent review (native sub-agent, 2026-11-17, commit e82490f57)

Verdict: APPROVE WITH CHANGES. This is the second review required for FM8 (80 section 7). Reviewed: `visibility.qnt` against 00 D26, 10 7.2, `compile/visible.rs` (alias scoping l.173 to l.228), and `tests/conformance_visibility.rs` (`check_state` compares pack facts, evidence files, entities, aliases and blocked counts). Checks: `quint typecheck` passes; `quint run` (seed 7, 6 steps) finds no violation for `inv` on the unmodified model.

Mutation checks (copies in /tmp, not committed), each run with `quint run --invariant=inv --max-samples=3000 --max-steps=6 --seed=7`:
- M1, withdrawn evidence counted (`liveEv` ignores `wd`): violation (caught by `invEvidenceOnlyInScope`).
- M2, scope ignored for facts (`scopedEv = liveEv`): violation.
- M3, compile keeps every touching fact (`hop1 = touch`, no visibility filter): violation.
- M4, human overlay facts pass the filter (`hop1` includes `human` facts): violation (caught).
- M5, alias scope ignored (`aliasScoped` without `.intersect(sc)`): **no violation** (`inv` holds on 627 traces/s). Blocking, see B1.

Blocking
1. **B1, the alias-scope invariants are vacuous.** `invAliasesInScope` and `invNoLeak` (through `supported`) and `invEntitiesSupported` call `aliasScoped` / `scopedEv`, the same helpers the compile uses. A mutation in a helper therefore changes the invariant and the compile together, and M5 survives. The header says the invariants "restate D26 from the unfiltered graph", but for aliases and for `supported` they do not. Fix: write the alias and entity invariants in direct terms over `g` and `scope`, for example `out.aliases.forall(a => g.aev.get(a).exclude(g.awd.get(a)).intersect(scope).size() > 0 and out.ents.contains(g.aowner.get(a)))` and `invEntitiesSupported` as `out.ents.forall(e => FACTS.exists(f => g.ev.get(f).exclude(g.wd.get(f)).intersect(scope).size() > 0 and ends(g, f).contains(e)) or ALIASES.exists(a => g.aowner.get(a) == e and g.aev.get(a).exclude(g.awd.get(a)).intersect(scope).size() > 0))`. Then rerun M1 to M5; all five must fail.
2. **B2, the scope rule for aliases is not in the mutation list of FM8.md section 4.** Add M5 and the other helper-level mutants to that list, so the review record shows they were tried. The conformance test is the only independent check of alias scoping in the real code (it compares `aliases` with the model on the bulk traces). State in FM8.md that the bulk trace run covers it, with the count.

Non-blocking
3. The model still collapses a file to one version; the rename case (`scope_versions` CTE in `pg.rs`) is outside the model, as FM8.md section 5 says. Keep the claim "the scope rule is per path" out of the safety statement until that is modelled or proved by the conformance test.
4. Blocked counts: the model and the code count over the whole Drive, while 10 7.2 says "one hop from the seeds". The model comment records this. Ask the spec owner to align 10 7.2 (spec 00 D26 says "blocked count", no hop limit) so the reviewer does not have to rely on the note.
5. The compile is run with a fixed graph per trace (`g` is constant in `step`). Changes of graph (new evidence, withdrawal between compiles) are the FM2 and FM3 territory; say so.
6. KP5 (Kani) proves `visible_decide` on indices. The model's `compile` is a second copy of that rule; the conformance test links the two. Keep both, and note that a helper mutation in the Rust `visible.rs` (not the kernel) is only caught by conformance.
7. Concern to re-check after B1: with a deterministic 4-file, 10-fact graph, the witnesses `witAliasFiltered` and `witScopeGrows` are the only ones that reach alias filtering; confirm they are true in the bulk run, not only in the simulation.
