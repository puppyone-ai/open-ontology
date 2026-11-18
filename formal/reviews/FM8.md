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
| M5 | blocked facts counted one hop from the seeds | `invBlockedCounts` |

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
