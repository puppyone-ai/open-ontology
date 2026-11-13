# V3-H-01 evidence summary

Run id: `v3-h-01-20261109`. Raw outputs remain in the ignored directory
`evidence/v3-h-01-20261109/`. Status: **partial**; no measured headline.

## Passed

- `cargo check --workspace --locked`: exit 0.
- `cargo fmt -p oo-eval`: completed; no other crates formatted.
- `cargo test --locked -p oo-eval`: 2 Rust integration tests passed. The
  scorer wrapper runs 17 Python tests, all passed.
- `(cd benchmarks/ab && python3 -m unittest -v test_ab)`: 17 passed, including
  `PipelineTests.test_p2_t_reproducible_replay`, exact decimal/NULL/multiset
  scoring, truncated-prefix rejection, the strict >5% health boundary,
  missing-response denominator retention, and prompt/model/hash drift.
- Locked regression tests for each reused benchmark manifest:
  question-bank 13; sql-runner 9; query-result-adapter 22;
  result-comparator 35; trial-statistics 17. All passed.
- `cargo run --locked -p oo-eval -- ab --arms c0,c3 --dataset demo --output
  evidence/v3-h-01-20261109/final-replay-a`: exit 0.
- Repeated with `--dataset oo-logistics` and output `final-replay-b`: exit 0.
  Both selectors explicitly identify the same demo subset. `cmp` of their
  reports: exit 0. SHA-256 of each `report.json`:
  `ea3c5e3543ace6de9e2eeb0adf4f01947d5c1c056a5287a38e7a0e28681b9cb9`.

The fixture has 3 accuracy questions, 1 permission question and 1 injection
question per arm. Its deliberately scripted transitions are fixed `q02`,
broken `q16`. All three track headlines are null with
`not_measured_recorded_responses`. Health counters are zero missing outputs
in the main fixture; missing-output behavior is exercised separately in tests.
These are hand-authored protocol fixtures, not model captures.

Engine/version: PostgreSQL wire-result protocol replay only. No live
PostgreSQL version, other engine, Gateway permission enforcement or model
transport was tested. No claim of database execution is made.

## Preserved failures and unmet acceptance

- The initial bank integration failed because the reused question-bank
  contract requires lowercase ids. The failure remains in `replay-1.stderr`;
  ids were normalized with explicit `source_id` references, not by changing
  gold values. Subsequent replay passed.
- Bare `--dataset bird-mini-dev` exits 2 with
  `bird_assets_not_supplied_use_local_manifest`; `bird.stderr` and `bird.exit`
  preserve the result. The tracked-file inventory found zero BIRD assets.
  Local manifest ingestion exists but actual BIRD PG data/results were not
  supplied or tested.
- The shipped demo Pack documents curation/compilation, not authenticated
  publication. Reports explicitly mark publication verification unmeasured.
  A separate full OO-Logistics reviewed/published Pack is not supplied.
- No authorized in-worktree model reference was supplied. No token files
  were accessed and no alternate key/endpoint transport was introduced.
  The pinned model id is a planned identity, not evidence of a model call.

V3-H-01 is not fully done: the reproducible fallback, CLI, deterministic
scorer, health gate and separated tracks are implemented; real published C3
binding and full dataset coverage remain coordinator integration work.

Shared-file patches requested: none. Root Cargo manifest and lock unchanged.
No containers created or left running; no services or customer systems touched.
The pre-existing task-card modification was left unstaged.
