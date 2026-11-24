# L2 formal evidence (spec 80 section 8)

Gate: FM1, FM2 (md subset), FM4 without counterexample; FM1 and FM4 conformance tests; KP1, KP2, KP8.
Run on 2026-11-17, macOS arm64 (16 cores), integration base `feat/super-ontology` 7834b28c2 merged into
`so/FV-laneA-integrated`. Toolchain pins: `formal/TOOLCHAIN.md` (Quint 0.33.1, Apalache 0.62.4, Quint Rust
evaluator v0.7.1, Kani 0.68.0 with CBMC 6.11.0, OpenJDK 21.0.13, JVM heap 8 GB).

Status: all three gate items green. Everything is bounded model checking or bounded proof; the bounds are
listed per item and nothing here is a claim beyond them.

## Models (commits of the `.qnt` files)

| Model | File | Last commit | Notes |
|---|---|---|---|
| FM1 job leases | `formal/quint/jobs.qnt` | a832368b4 | 3 workers, 4 jobs, max_attempts 3, crash limit 2, generation cap 6; parse jobs quarantine at the attempt cap (0138 SQL) |
| FM2 file state machine | `formal/quint/file_lifecycle.qnt` | b1358f6af | extended from the md subset to all 16 states (the L3 form); this covers the md subset |
| FM4 event outbox and catch-up | `formal/quint/event_stream.qnt` | a832368b4 | gap free seq, `since` replay, duplicate drop, 410 refetch, transient events |
| (FM3 sync, L3) | `formal/quint/sync_reconcile.qnt` | 33026e011 | reported in L3/formal.md |

Reviews: `formal/reviews/FM1.md`, `FM2.md`, `FM4.md` (model to spec to code mapping and the findings).

## Apalache (`quint verify`), FAST bound, one invariant per run, one Apalache process at a time

Command: `formal/scripts/formal-verify-fast` (loops every model and every invariant of its `vinv` header, each in
its own work directory). All 27 (model, invariant) pairs of the four models: `[ok] No violation found`. Wall seconds
per pair (the run shared the machine with Rust test builds, the container replays and `quint run` trace
generation, so these are upper bounds). The CI budget is 15 minutes (900 s) per pair; the maximum is 425 s.

| Model | max-steps | Invariant | Seconds |
|---|---|---|---|
| jobs (FM1) | 8 | invLeaseShape | 86 |
| | | invSingleLiveLease | 11 |
| | | invGenerationMonotone | 40 |
| | | invStaleRejected | 9 |
| | | invTerminalAbsorbing | 35 |
| | | invCancelTerminal | 91 |
| | | invCancelProvenance | 66 |
| | | invQuarantineAfterCrashes | 40 |
| | | invNoLostJob | 407 |
| | | invAtMostOnceSuccess (row level) | 120 |
| file_lifecycle (FM2) | 8 | invOnlyTableTransitions | 31 |
| | | invPublishedFromLatest | 34 |
| | | invNoStaleOverwrite | 320 |
| | | invDuplicateSameHash | 85 |
| | | invNoDeadEnd | 32 |
| | | invQuarantineShape | 165 |
| | | invQuarantinePoison | 85 |
| event_stream (FM4) | 10 | invOutboxGapFree | 12 |
| | | invViewMatchesCursor | 425 |
| | | invCaughtUpEqualsServer | 126 |
| | | invTransientHasNoSeq | 10 |
| sync_reconcile (FM3, L3) | 8 | six invariants | 35 to 352 (see L3) |

Deep (nightly) bound: `deep-steps` in each model header (FM1 and FM2 and FM3: 10, FM4: 12), run by
`formal/scripts/formal-verify-deep`. NOT run for this report: the nightly job has not been measured at the deep
bound on this machine (earlier measurements: FM1 invNoLostJob took 40 minutes at 10 steps).

Counterexamples found and how they were handled:
- FM1 (card B-S3-04): reading 10 section 4.1.6 against the SQL showed that a parse job at its attempt cap was
  failed, not quarantined. The SQL (0138, parse jobs only) and the model were changed to quarantine at the cap;
  `jobs_live::the_parse_attempt_cap_ends_in_quarantine` locks it and `conformance_jobs` fails on the old SQL.
- FM1: `invCancelProvenance` added after the independent review; mutation check (`ack_cancel` ignoring the flag)
  is caught by `quint run`.
- FM2: spec finding only: the 10 section 4.2 table has no `stored -> parse_failed` row although 4.3.2 and
  `state.rs` have it (table to be extended). No counterexample.
- FM4: none.

## Simulation (`formal/scripts/formal-simulate`, `quint run`, 10,000 traces of 40 steps per model, fixed seeds)

`formal-simulate: 4 model(s) ok`, no violation. Witness coverage (traces of 10,000 that reached the event):
FM1 witSucceeded 2061, witCancelled 6180, witQuarantined 47, witReclaimed 5975, witCapQuarantined 537,
witFailed 2007, witBusyGiveBack 1476. FM4 witCaughtUp 8820, witGone410 3998, witDuplicateDropped 9204,
witTransientSeen 10000, witReconnectReplay 7344. FM2: all 16 `witState_*` reached (rarest: witState_duplicate
25, witState_extract_partial 138, witState_published 314), witQuarantined 1989, witQuarantineRetried 743,
witOutdatedReparse 109. Wall time 4 min 37 s for the four models.

## Conformance (model traces replayed on the real system)

Container: `postgres@sha256:839fbb5a4942a7758dd93e18b42ea5649807491a0545b5eeeb8f0b8bb11857a5`,
`-c track_commit_timestamp=on`, label `oo.lane=super-ontology-fv`, removed afterwards.

| Model | Test | Committed samples | Bulk replay |
|---|---|---|---|
| FM1 | `conformance_jobs` (real `_oo_docs.*` job functions) | 20 traces, 600 steps | 10,000 traces, 300,000 steps, all conform (49.6 s) |
| FM4 | `conformance_events` (event log and the SSE endpoint) | 12 traces, 342 steps | 1,000 traces, 28,398 steps, all conform (50.4 s) |

Not covered by the replays: parallel sessions in one database (`jobs_live.rs` covers concurrency), backoff
and `run_at` (delay 0), priority, kinds, claim limit above 1; FM4 does not replay the renderer side
(`tests/unit/super-drive/conformance`, a Desktop card). Liveness (jobs reach a terminal state, the client
reaches head) is not model checked (no TLC): only the safety forms above and reachability witnesses.

## Kani (KP1, KP2, KP8), `cargo kani`, one harness at a time

All harnesses: `VERIFICATION:- SUCCESSFUL`. Times are CBMC time.

| KP | Harness | Bound | Time |
|---|---|---|---|
| KP8 | `cas::proofs::kp8_cas_path_is_a_legal_name` | symbolic 64 hex bytes, all four kinds | 21 s |
| KP8 | `cas::proofs::kp8_cas_path_is_injective` | same | 23 s |
| KP8 | `cas::proofs::kp8_bad_keys_are_rejected` | same | 12 s |
| KP1 | `ingest::proofs::kp1_raw_key_safe_stays_inside_root` | any bytes, up to 12 | 183 s |
| KP1 | `ingest::proofs::kp1_docpath_stays_inside_root` | any bytes, up to 12 | 293 s |
| KP2 | `docir::proofs::kp2_a1_roundtrip` | every column A..ZZZ and row 1..9,999,999 | 32 s |
| KP2 | `docir::proofs::kp2_a1_write_rejects_out_of_range` | all u32 | 0.1 s |
| KP2 | `docir::proofs::kp2_a1_parse_is_total_and_sound` | any bytes, up to 8 | 56 s |
| KP2 | `docir::proofs::kp2_docx_check_is_total` | all u32 pairs | 0.2 s |

How they are proven (kernel approach): CBMC does not finish through `String`, `format!`, `chars()` or `split`
over symbolic input, so each property is proven on an allocation free byte kernel that the production function
calls: `write_rel_path` (KP8, called by `object_rel_path`), `check_doc_path_bytes` (KP1, called by `DocPath::new`),
`a1_write` and `a1_parse_bytes` (KP2, called by `a1_from_cell` and `a1_parse`). The wrappers only copy bytes to
and from `String` or `&str`. The kernel and the old rules were compared with 4 million random strings (KP1) and
18,300 columns by 10 rows plus 3 million random strings (KP2) before the change; the tests are not kept.

What is NOT covered by KP1, KP2, KP8:
- KP1: `normalize_raw_key` (UTF-8 decoding and the NFC tables); bounded to 12 bytes (a longer input is only
  covered by the kernel being a left to right scan, not by the proof); `DocPath::new` itself is a wrapper over the
  proven kernel; mount target and `suggest/` collection paths are not separate code in this build.
- KP2: `Anchor::fragment` and the `format!` wrappers (a one digit text fragment round trip ran 10 minutes without
  a result); `Anchor::check` of the pdf, slide, text and cells variants through a symbolic enum (the pdf comparison
  alone ran over 8 minutes; the code is three comparisons and has unit tests); `rc_parse` (docx cell ranges) and
  sheet name quoting. So "parse(format(a)) == a" is proven for the A1 cell codec, not for every anchor kind.
- KP8: the BLAKE3 digest itself; injectivity is on the hex key to path map, as the spec states.

Patches the coordinator must apply for the proofs to build (`patches/`): `V-S2-03-oo-core-docir-proofs.patch`
(oo-core `build.rs` that declares the `kani` cfg, `docir/mod.rs`, `docir/anchor.rs` kernels, `docir/proofs.rs`)
and `V-S2-03-objectstore-docpath-kernel.patch` (`DocPath::new` calls the kernel). Without them `cargo kani -p
oo-core` has no harness and `DocPath::new` uses the old equivalent rules.

## Not covered at L2 (stated plainly)

No liveness checking anywhere; FM1 concurrency across database sessions; the nightly deep bound; the KP3 and
KP4 harnesses (L3); no claim that the models equal the SQL beyond the replayed traces and the reviews.
