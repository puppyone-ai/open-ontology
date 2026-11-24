# L3 / L4 formal evidence (spec 80 section 8, with D81)

Gate (D81): typecheck + simulate + verify-fast + conformance + kani-fast for what exists. `formal-verify-deep` is **not a gate** (informational results in
section 6). Run on 2026-11-17/18, same machine and pins as `evidence/L2/formal.md` and `formal/TOOLCHAIN.md` (Quint 0.33.1, Apalache 0.62.4, Kani 0.68.0 /
CBMC 6.11.0, JVM heap 8 GB; never more than two Apalache processes, each on its own server port, and no Kani next to Apalache). Branch `so/FV-laneA-L4`.
Container `postgres@sha256:839fbb5a...857a5`, `track_commit_timestamp=on`, label `oo.lane=super-ontology-fv`, removed afterwards.

Result: **green** for the models and proofs below. Not done or not covered: section 8.

## 1. What the formal work found in the implementation

Nine defects of S3/S5/S6 code, each found by replaying a model against the real code, each fixed in its own commit with a `Formal:` trailer and delivered as a patch.

| # | Model | Defect | Fix |
|---|---|---|---|
| 1 | FM5 | `withdraw_evidence` retracted a fact a person had accepted when its file went away (D22) | 4f0a702e6 |
| 2 | FM5 | `sweep_orphans` queued an orphan item for an overridden assertion and re-orphaned a kept one: overlay orphaned, no item for anybody | 4f0a702e6 |
| 3 | FM5 | declining an orphan left a published fact whose source is gone | 4f0a702e6 |
| 4 | FM5, FM11 | `enqueue` retired pending `supersede_candidate` items the temporal pass still held: a held fact waited for nobody | 4f0a702e6 |
| 5 | FM11 | a `rejects` and a `decides` of one alternative in one run both published | 94f21fc1a |
| 6 | FM10 | corrections never counted as pending in `covers_drive_rev`: a task started right after one compiled without it and reported complete | 2d81854ee |
| 7 | FM6 | a share joining a task group at the group's older version never got the entries of the versions in between; `check` answered 0 over a retracted fact | c46f77b20 |
| 8 | FM4 | (finding, no patch) the live stream loop would silently skip purged events for a connected client behind a retention purge; no job purges events yet | review FM4 |
| 9 | FM2 | (finding, no patch) `state::transition` has no version fence (`version_id` is only logged); the fence is the stage's SQL | review FM2 |

Readings of the spec the models made explicit (no patch): blocked counts are over the whole Drive in the code, not "one hop from the seeds" (FM8); `require_complete` is stricter than
`min_drive_rev` (FM10); stored `covers_drive_rev` is a lower bound (FM10); a constraint violation names the first conflicting pair only (FM11); the D58 retention job is not implemented and
`pinned_by` is a view (FM6); `PATCH /shares/{sid}` (contract G6, extend `expires_at`) answers 405 (FM6); a share past its end date but not swept gets no UPDATES entry, which is harmless and now in the model (FM6).

## 2. Models and reviews

| Model | File | FAST bound | Pairs | Reviews |
|---|---|---|---|---|
| FM1 jobs | `jobs.qnt` | 8 | 10 | `FM1.md` (unchanged in this lane; L2 evidence) |
| FM2 file lifecycle (lease generation added) | `file_lifecycle.qnt` | 8 | 8 | `FM2.md` + independent review answered |
| FM3 sync (fs) | `sync_reconcile.qnt` | 8 | 7 | `FM3.md` + independent review answered |
| FM4 event stream | `event_stream.qnt` | 10 | 5 | `FM4.md` + independent review answered |
| FM5 review decisions | `review_decide.qnt` | 7 | 9 | `FM5.md` + independent review answered |
| FM6 share lifecycle | `share_lifecycle.qnt` | 5 | 12 | `FM6.md` + independent review answered |
| FM8 visibility | `visibility.qnt` | 2 | 11 | `FM8.md` + second review answered (B1 fixed) |
| FM10 freshness token | `freshness_token.qnt` | 8 | 10 | `FM10.md` + independent review (approve) |
| FM11 constraints | `constraints.qnt` | 4 | 8 | `FM11.md` + independent review answered |

Bounds are in the model headers (`verify-steps`); a pair listed in a header `dinv` is not in the FAST set (section 6).

## 3. typecheck and simulate

`formal/scripts/formal-typecheck`: `9 model(s) ok`. `formal/scripts/formal-simulate` (10,000 traces per model, fixed seeds, 4 shards, all witnesses): `formal-simulate: 9 model(s) ok`,
5:27 wall. Mutation checks of the new models (30,000 traces, copies in `/tmp`): FM8 8, FM5 7, FM10 4, FM11 6, FM6 10, FM3 2, FM4 1, FM2 3 mutants, each caught by the invariant named in the review
(one FM2 mutant, generation alone instead of generation and version, is equivalent to the model: see `FM2.md`). Two FM5 mutants were first missed (operator precedence in the invariant, and a flag read from the mutated
slot); both invariants were rewritten.

## 4. Apalache, FAST bound (`formal-verify-fast`), one invariant per run, seconds

All `[ok] No violation found`; every pair inside the 900 s budget. Pairs were run two at a time, each on its own server port (the timings are under that load).

- FM8 (2 steps): invFactsInScope 70, invEvidenceOnlyInScope 89, invEntitiesSupported 175, invAliasesInScope 80, invNoLeakAliases 73, invNoLeakFactEnds 99, invHumanNoBackdoor 65, invBlockedFactCount 81, invScopeMonotone 134, invOutputWithinInput 214, invBlockedFactsNotVisible 61.
- FM5 (7 steps): invEvidenceGate 59, invBySourceAsOf 426, invNoDoubleDecide 446, invIfMatch 40, invRevMonotone 212, invLlmNeverAuto 592, invInUseNotice 491, invRejectWins 663, invOrphanQueued 865.
- FM10 (8 steps): invRevGapFree 68, invCoversMonotone 62, invLiveCoversSound 29, invCoversSound 46, invCoversMax 28, invCompileWaits 21, invCoverageHonest 154, invCoverageInScope 20, invRequireComplete 18, invOverlayAdvances 48.
- FM11 (4 steps): invBanHolds 169, invNoCurrentViolation 267, invHeldHaveConflict 226, invPartialBlocking 201, invHeldHaveViolation 267, invCertainNotViolation 215, invPackNeverHeld 151, invWellFormed 198.
- FM6 (5 steps): invCredentialDead 238, invRefreshAtomic 339, invPackBytesImmutable 194, invDriftMarked 299, invChangeDelivered 483, invCheckCodes 184, invCheckFailClosed 108, invGroupSameVersion 252, invGroupAllOrNothing 157, invRefreshPolicy 295, invRetention 308, invExtendKeepsContent 147, invTerminalAbsorbing 169.
- FM4 (10 steps): invNoStrandedClient 7, invOutboxGapFree 12, invViewMatchesCursor 425, invCaughtUpEqualsServer 123, invTransientHasNoSeq 10.
- FM3 (8 steps): invOnlySweepKills 38, invSweepOnlyUnseen 65, invMassHold 37, invGenerationMonotone 51, invHeldAfterMassOnly 47, invRenameCarries 72, invConverged 475.
- FM2 (8 steps): invResultFromCurrentLease 152, invOnlyTableTransitions 36, invPublishedFromLatest 45, invNoStaleOverwrite 403, invDuplicateSameHash 104, invNoDeadEnd 43, invQuarantineShape 197, invQuarantinePoison 116.
- FM1 (`jobs`): not re-run in this lane (model and code unchanged since L2); see `evidence/L2/formal.md`.

Pairs that passed but were over 900 s at the bound, so are in `dinv` and not in the FAST set: FM5 invThreshold 1,561 s and invOverlayNeverRetracted 1,044 s (7 steps), FM6 invUpdatesAppendOnly 1,045 s (5 steps).
FM11 at 5 steps and FM8 at 3 steps did not fit; the bounds are 4 and 2.

## 5. Conformance (live PostgreSQL where noted; committed traces `formal/traces/<model>/`)

| Model | Test | Result on the final code |
|---|---|---|
| FM1 | `conformance_jobs` | 20 traces, 600 steps |
| FM2 | `conformance_state` (table equality, probes, `file_version` rows per model version, the stage fence on every `finishParse` step) | 12 traces, 334 steps, 3,380 refused-transition probes; bulk run earlier: 300 traces, 8,344 steps, 84,036 probes |
| FM3 | `conformance_sync` | 12 traces, 480 steps |
| FM4 | `conformance_events` (follows the model on purge) | 12 traces, 480 steps |
| FM5 | `conformance_review` (facts, queue, decide, by source, orphans, corrections) | 8 traces, 320 steps; bulk earlier: 300 traces, 12,000 steps |
| FM5 D51 | `conformance_review_d51` (no database: `certain_supersedes` and `classify` against the model's supersession slots) | passes; bulk: 14,736 supersession slots |
| FM6 | `conformance_share`: ITF replay on the live engine (mount, publish, refresh, group refresh and its rollback, check and at-checkpoint, revoke, expiry, refs credentials; state, version, policy, `refreshed_from`, UPDATES count, check codes, credential validity compared at each step) plus three scenarios | replay 8 traces, 320 steps; bulk earlier: 40 traces, 1,550 steps (2 ended where the model had no free share slot) |
| FM8 | `conformance_visibility` (no database: `VisibleGraph::load`, one-hop pack facts, evidence files, entities, aliases, both blocked counts, whole packs scanned for out-of-scope names) | passes; bulk earlier: 1,000 traces, 7,000 states, 19,627 pack facts |
| FM10 | `conformance_freshness` | 8 traces, 320 steps; bulk earlier: 150 traces, 6,000 steps |
| FM11 | `conformance_constraints` | 8 traces, 226 steps; bulk earlier: 300 traces, 8,419 steps (4 ended at a valid outcome other than the model's, 28 where the model resolves a fact the engine's item does not name) |

## 6. Deep bound (informational: not a gate per D81)

`formal-verify-deep` was started and then stopped when D81 removed it from the gate. Finished before the stop, all `No violation found`: FM8 (3 steps) invFactsInScope 92 s, invEvidenceOnlyInScope 116,
invEntitiesSupported 321, invAliasesInScope 104, invNoLeakAliases 104, invNoLeakFactEnds 178, invHumanNoBackdoor 91, invBlockedFactCount 123, invScopeMonotone 231, invOutputWithinInput 349; FM6 (6 steps) invCredentialDead 723 s.
Not finished: FM8 invBlockedEntityCount at 3 steps (running 60 min when stopped); invBlockedHidden and invNoLeakEntities were not run at 3 steps (more than 1,000 s at 2 steps). Everything else was not run. Not covered by Apalache:
invBlockedEntityCount (checked by simulation, the replay and unit tests; a set-and-size reformulation also exceeded 900 s and was removed), see `FM8.md`.

## 7. Kani (`kani-fast`: `cargo kani -p oo-docs`, `cargo kani -p oo-core`)

`cargo kani -p oo-docs`: `Complete - 20 successfully verified harnesses, 0 failures`. `cargo kani -p oo-core`: `Complete - 4 successfully verified harnesses, 0 failures`. One process at a time, seconds:

| KP | Harnesses (all SUCCESSFUL) | Time |
|---|---|---|
| KP1 | `ingest::proofs::kp1_docpath_stays_inside_root`, `kp1_raw_key_safe_stays_inside_root` | 294, 192 |
| KP2 | `docir::proofs`: `kp2_docx_check_is_total`, `kp2_a1_parse_is_total_and_sound`, `kp2_a1_write_rejects_out_of_range`, `kp2_a1_roundtrip` (oo-core) | 0.3, 54.6, 0.1, 38.2 |
| KP5 | `compile::proofs::kp5_visible_decide_is_d26`, `kp5_out_of_scope_evidence_shows_nothing` (the D26 kernel: output is a subset of the input, exact for facts and aliases, an entity iff something visible points at it) | 4.0, 0.8 |
| KP6 | `kp6_rank_order_is_total` (total, antisymmetric, transitive, ties only identical keys), `kp6_token_estimate_is_subadditive` (class streams of up to 4+4), `kp6_next_level_is_monotone`, `kp6_degradation_step_loses_exactly_one_level`, `kp6_degradation_stops_only_at_the_floor` | 25.6, 8.6, 0.01, 1.2, 0.3 |
| KP7 | `state::proofs::kp7_state_names_round_trip`, `kp7_every_state_has_a_way_out`, `kp7_transition_function_matches_the_table` | 0.8, 337, 1.2 |
| KP8 | `cas::proofs::kp8_bad_keys_are_rejected`, `kp8_cas_path_is_injective`, `kp8_cas_path_is_a_legal_name` | 13.3, 24.0, 20.5 |
| KP10 | `kp10_label_first_front_matter_exact_body_untouched_{1111,2212,1221}`, `kp10_blank_lines_at_the_end_collapse`, `kp10_prefix_is_deterministic` | 101, 126, 100, 82, 118 |

KP3 and KP4: not done (section 8). Method: Kani terminates only on allocation-free code, so the code calls small kernels and the harnesses prove those (see the L2 and L3 reports for KP1, KP2, KP7, KP8;
`compile/kernels.rs` and `tokens_of_classes` for KP5, KP6, KP10). KP10 lengths are fixed per harness (1 to 2 symbolic bytes): a harness over symbolic lengths did not finish in 10 minutes; the token-estimate harness on the real
`&str` function did not finish in 2 minutes (unicode tables), which is why the class kernel exists.

## 8. Not done, stated plainly

- KP3 (S3 sequencer comparison) and KP4 (retry backoff and lease time): no Rust code to prove (backoff and lease are SQL, no S3 sequencer in this tree). Not done.
- FM7, FM9, KP9, FM3 for S3 and multi-cloud, and the L8 re-run: dropped by D81 (covered by tests), not started.
- FM6 is replayed for add-only publishes; drift and block entries (retract, supersede, violation) are covered by three scenario tests and the model, not by the replay (they depend on the share's `uses`).
  `extend` is model-only (G6 not implemented). The D58 retention job does not exist.
- FM5 merges (kind 1 of the model) are covered by `resolve_live.rs`, not replayed. FM11 kind 2 (identity) is model-only in the replay profile.
- FM8: the SQL scope reader is exercised by `gates_live.rs` and `share_live.rs`; the model graph is replayed through the in-memory reader. `invBlockedEntityCount`, `invBlockedHidden` (conjunct by conjunct covered, see `FM8.md`) and `invNoLeakEntities` (= `invEntitiesSupported`) are not in the Apalache set.
- Kani proves allocation-free kernels the code calls (`compile/kernels.rs`, `tokens_of_classes`, the `state` and `cas` kernels), not the string handling around them; KP10 lengths are fixed per harness.
- The independent reviews are done (merged in `formal/reviews/FM*.md`, answered in this lane); FM8 had two reviewers.
