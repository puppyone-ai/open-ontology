# FM5 review: batch review decisions (`formal/quint/review_decide.qnt`)

Model of the review queue and its decisions as implemented by `crates/oo-docs/src/review/{queue,decide,overlay}.rs`,
`ingest/files.rs::withdraw_evidence` and the publisher (`publish/run.rs`). Spec: 10 section 4.8, 5.3, 8 (`review:decide`),
00 D22, D24, D51; 80 section 3.1 (FM5). Card V-S5-01.

Review status: author self-review done (lane A, 2026-11-17). The 80 section 5 step 2 review by the board lead plus a
native sub-agent is **not yet done**; this document is the input for it.

## 1. Model shape

- A slot is one candidate: a draft fact (kind 0), an LLM entity merge (1), a supersession (2), or an "orphaned overlay"
  item (3). It has a fact side (`fst`: none, draft, eligible, published, rejected, retracted) and an item side (`ist`: none,
  pending, accepted, rejected, superseded) with `rev`, the event `seq` it appeared at, how it was decided (`via`, `asOf`),
  `executed` (a merge or supersession took effect), `notice` (D51) and the overlay (`ov`: none, assert, reject, active,
  orphaned, withdrawn). `evid` says some live evidence remains (DOC-G2).
- Bounds: 5 slots, 2 sources, revision up to 3, any interleaving of arrival, rescoring, decisions, accept by source,
  publish runs, source deletions and corrections.
- Profile `stepFacts` (fact candidates only) is what `conformance_review.rs` replays; merges and supersessions stay in the
  default `step` and are covered by the resolver and temporal tests.

## 2. Spec rule to model to implementation

| Rule | Implementation | Model | Checked by |
|---|---|---|---|
| D24 thresholds: >= auto bp publishes, queue floor <= bp < auto waits, below stays a draft (not queued) | `extract/confidence.rs`, `queue::classify` | `appearKind` (conf class 2 / 1 / 0) | `invThreshold`, witnesses, conformance (`classify_drafts`) |
| accept by source only touches items that existed at `as_of_seq` | `decide_by_source` (`created_at <= event.at`) | `bySource` (`seq <= ao`) | `invBySourceAsOf`, `witBySourceSkippedLate` |
| `If-Match`: a stale revision changes nothing; a decided item is `already_decided` | `decide_items` (one transaction per item) | `decide` result `ok / conflict / already_decided / not_found` | `invIfMatch`, `invNoDoubleDecide`, `invRevMonotone` |
| D51: a merge or supersession that is not rule-certain only runs after a person | `temporal`, `resolve` (queue as `supersede_candidate` / `entity_merge`) | `appearKind` (merge / supersession), `decide` accept | `invLlmNeverAuto` |
| D51: a rule-certain supersession of a fact a live share used leaves one notice | `temporal/pg.rs` (`supersede_in_use`) | `notice` on `appearKind` | `invInUseNotice` |
| overlay precedence: `fact_reject` beats threshold and assertion | `overlay_current`, `classify` (`rejected` before `asserted`) | `markWrong`, `decide` reject | `invRejectWins` |
| D22: a person's overlay is never retracted by the engine; evidence loss queues an orphan item | `withdraw_evidence`, `overlay::sweep_orphans`, `decide` (orphaned_overlay) | `evidenceGone`, orphan items, `orphanFix` | `invOverlayNeverRetracted`, `invOrphanQueued` |
| DOC-G2: a fact with no live evidence is not published (an overlay fact waits as an orphan) | `publish/gates.rs` | `evid`, `publish` | `invEvidenceGate` |
| 4.8: a retracted or overridden fact retires its pending item as `superseded` | `queue::enqueue` | `markWrong`, `evidenceGone` | conformance |

## 3. What the model found in the implementation (all patched, patch files listed in the L4 report)

1. `ingest::files::withdraw_evidence` retracted a fact a person had accepted (overlay `fact_assert`) when its file went
   away: it only spared `origin = 'human'`. D22 says a person's overlay is never retracted; the fact must stay and the overlay
   become an orphan. Fix: skip facts with an active or orphaned `fact_assert` / `fact_edit` overlay.
2. `overlay::sweep_orphans` queued an orphan item for an assertion the person had since overridden with `fact_reject` (fact
   already retracted), and queued it again for an overlay the person had just kept (`orphan_decision`): the dedupe key was
   already used, so the overlay was `orphaned` with no pending item for anybody. Fix: only live facts without an active
   `fact_reject`, and not an overlay that carries `orphan_decision`.
3. Declining an orphan (`decide` reject on `orphaned_overlay`) withdrew the overlay and left a published fact whose source is
   gone. Fix: the fact is retracted when no live evidence is left.
4. `queue::enqueue` retired pending `supersede_candidate` items whose fact the temporal pass still holds; `queue_held` then
   hit its dedupe key and queued nothing, so the held fact waited for nobody (found by FM11, same fix). Fix: keep the item
   while the fact is still a draft.

## 4. Mutation checks (simulation, 30,000 traces, `/tmp` copies of the model; the invariant that catches each)

| # | Mutation | Caught by |
|---|---|---|
| M1 | accept by source ignores `as_of_seq` | `invBySourceAsOf` |
| M2 | a stale revision is applied | `invRevMonotone` |
| M3 | a decided item can be decided again | `invNoDoubleDecide` |
| M4 | a supersession that is not rule-certain runs at once | `invLlmNeverAuto` |
| M5 | a source deletion retracts a fact that carries an overlay | `invOverlayNeverRetracted` |
| M6 | the engine publishes a fact without evidence | `invEvidenceGate` |

Two of these were first missed (M3, M4): an `A and B implies C` without parentheses had moved `A` into the antecedent, and the
certainty flag was read from the slot the mutation itself changed. Both invariants were rewritten; M3 and M4 are caught now.

## 5. Not modelled

`review:undo` (not implemented), `edit` decisions, the 2 s publish debounce, entity merge execution (resolver tests),
idempotency keys, event payloads. Liveness (every decision ends in a version) is shown by witnesses only.

## 6. Conformance

`crates/oo-docs/tests/conformance_review.rs` (live PostgreSQL): see the L4 report for trace and step counts.
`markWrong` is the SQL of `routes_correct.rs` (the handler is HTTP-only).
