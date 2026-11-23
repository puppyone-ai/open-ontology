# L3 formal evidence (spec 80 section 8)

Gate: FM2 full state machine, FM3 (fs and s3) without counterexample and conforming; KP3, KP4, KP7.
Run on 2026-11-17 together with the L2 report (same machine, same pins: see `evidence/L2/formal.md` and
`formal/TOOLCHAIN.md`), integration base `feat/super-ontology` 7834b28c2 merged into `so/FV-laneA-integrated`.

Status: FM2 (full) and FM3 (fs backend) are green; KP7 is green. NOT green yet: FM3 for the s3 backend (not
built), KP3 and KP4 (no code to prove). The L3 formal gate is therefore PARTIAL, as listed under "Not covered".

## Status under D81 (2026-11-18, supersedes the PARTIAL status above)

D81 narrows the L3 formal gate to the fast checks of models that exist (80 section 8 as amended). On the final
models (`evidence/L4/formal.md`, integration `feat/super-ontology` 724e7915e): FM2 and FM3 (fs) pass FAST,
`conformance_state` and `conformance_sync` pass, KP7 passes. FM3 for s3 is dropped by D81; the s3 behaviour is
covered by the L3 acceptance s3 cells. KP3 and KP4 are not done (no Rust code to prove), listed as such.
L3 formal gate under D81: green.

## Models (commits of the `.qnt` files)

| Model | File | Last commit | Bounds |
|---|---|---|---|
| FM2 file state machine, all 16 states | `formal/quint/file_lifecycle.qnt` | b1358f6af | 3 files, 3 versions; FAST 8 steps, deep 10 |
| FM3 incremental sync, fs backend | `formal/quint/sync_reconcile.qnt` | 33026e011 | 4 keys, 3 contents, at most 6 source operations, 1 partial listing, 2 targeted syncs, `mass_delete_min` 1; FAST 8 steps, deep 10 |

Reviews: `formal/reviews/FM2.md`, `formal/reviews/FM3.md`.

## Apalache, FAST bound, one invariant per run, sequential

FM2 (`file_lifecycle`, 8 steps), seconds: invOnlyTableTransitions 31, invPublishedFromLatest 34,
invNoStaleOverwrite 320, invDuplicateSameHash 85, invNoDeadEnd 32, invQuarantineShape 165, invQuarantinePoison 85.
FM3 (`sync_reconcile`, 8 steps), seconds: invOnlySweepKills 35, invSweepOnlyUnseen 36, invGenerationMonotone 44,
invHeldAfterMassOnly 41, invRenameCarries 52, invConverged 352. All `[ok] No violation found`; the 15 minute
budget per pair holds with a wide margin (max 352 s here). Timings were taken under shared load, see L2.
`invConverged` is the safety form of "one reconcile makes the index equal the folder". The nightly deep bound
(10 steps) was NOT run for this report.

Counterexample found and handled: FM3-001 (found by the model). A failed, held or interrupted sync of generation
g leaves `seen = g` on every row it listed, and the retry is again generation g, so a file deleted in between was
not swept. With `RETRY_RESETS_MARKS = false` `invConverged` has a 7 step counterexample
(`formal/traces/regressions/FM3-001.itf.json`). Reproduced on the real code by
`ingest_fs::a_retried_generation_forgets_the_marks_of_the_failed_attempt` (fails without the fix); fixed in
commit 38632493d (`start_sync` forgets the marks of a generation that is started again). FM2: one spec finding
(10 section 4.2 table lacks `stored -> parse_failed`), no counterexample.

## Simulation

`formal/scripts/formal-simulate` (10,000 traces of 40 steps, fixed seeds): `4 model(s) ok`. FM3 witnesses: witRename
384, witHeld 1652, witConfirmedSweep 1568, witPartialKeepsDeleted 1849, witTargetedCreated 1622,
witRecreatedDuringListing 349, witRetrySameGeneration 10000. FM2 witnesses: see the L2 report (all 16 states).

## Conformance

Container as in L2 (`postgres@sha256:839fbb5a...857a5`, `track_commit_timestamp=on`, label
`oo.lane=super-ontology-fv`, removed afterwards).

| Model | Test | Committed samples | Bulk replay |
|---|---|---|---|
| FM2 | `conformance_state`: every step runs the real state transition function and the real `_oo_docs` state rows; refused transitions probed | 12 traces, 345 steps, 3,483 refused-transition probes | 500 traces, 14,347 steps, 143,446 probes, all conform (70.9 s) |
| FM2 table | `conformance_state::fm2_transition_table_equals_is_legal` (no database): the model's `EDGES` equals `is_legal` for all 16 x 17 pairs | runs in `cargo test` | n/a |
| FM3 | `conformance_sync`: every step runs the real `reconcile` on a real temporary directory (hash jobs are run by the model's `hashAll` step) and compares the index with the model | 12 traces, 480 steps | 500 traces, 20,000 steps, all conform (54.2 s) |

Bulk sets are generated with `formal/scripts/formal-traces` (seed 0x464d) and are not committed.

## Kani

| KP | Harness | Result | Time |
|---|---|---|---|
| KP7 | `state::proofs::kp7_transition_function_matches_the_table` (all state pairs against a second statement of the 4.2 table; total, no panic) | SUCCESSFUL | 1.1 s |
| KP7 | `state::proofs::kp7_every_state_has_a_way_out` | SUCCESSFUL | 312 s |
| KP7 | `state::proofs::kp7_state_names_round_trip` | SUCCESSFUL | 0.8 s |
| KP3 | S3 sequencer comparison | NOT DONE | no S3 sequencer code exists in this tree (lane B, objectstore-s3) |
| KP4 | retry backoff and lease time arithmetic | NOT DONE | the backoff and the 30 s lease are computed in SQL (`0138_docs_sources_jobs.sql`), there is no Rust function to prove; a harness would have to prove a copy |

KP7 covers `is_legal` and the state name round trip; it is the same table as FM2 `EDGES` by the table conformance
test above, so model, code and the independent statement agree for all pairs.

## Not covered at L3 (stated plainly)

- FM3 for s3 (sequencer ordering, versioned buckets, multi cloud events): the backend is not built; nothing
  here says anything about it.
- KP3 and KP4: no harness (see table). To close them: write the sequencer comparison and the backoff as pure
  Rust functions when the s3 backend lands, then add the harnesses.
- FM3 model limits: pages (a listing is one step), a listing that changes while it is read, symlinks, invalid or
  colliding names, `max_files`, sniffing and sizes, hash backlog pause, parallel syncs (the drive lock serialises
  them), the scheduler; a write between two hash jobs of one drain.
- Liveness (the index converges, no file stays in an intermediate state) is not model checked (no TLC): only the
  safety forms `invConverged` and `invNoDeadEnd`.
- The nightly deep bound; conformance only for the traces replayed (counts above), not the whole state space.
