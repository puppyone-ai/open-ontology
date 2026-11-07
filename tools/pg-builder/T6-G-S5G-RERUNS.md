# G-S5g: last four dialogue reruns

STATUS: partial. S5-G-01 remains not done. All four authorized versions and
their one permitted retry ran independently. None achieved a new standalone
20/20 result. No answers were combined, and no gold, prompt, scoring, model
budget or timeout was changed.

## Cleanup diagnosis and correction

Historical Oracle run `g-s5c-e309-oracle-20261105` answered 20/20 but failed
before cleanup completed. Its `0044.json` records exit 1 for the target
container's Docker ownership inspection; `0044.stderr` reports inability to
connect to the Docker daemon. The result and command files were verified
against the original SHA256SUMS. The old result remains failed.

`tools/oo-test/t1.py` now applies the existing bounded pre-dispatch transport
retry to read-only Docker inspections: three attempts, retaining every raw
command log. Ownership labels must still match before removal. Permission
errors, partial output, missing objects and removal commands are not retried.
Persistent refusal still fails. Regression tests exercise these boundaries.
This addresses transient inspection refusal, not arbitrary Docker outages.

Both new Oracle runs completed cleanup successfully, with empty cleanup
failure lists and zero containers left. This is live cleanup evidence, not a
claim that the historical socket outage was reproduced.

## Runs and results

Rebased on `1cbfc5a3c`; tested clean revision `fafe7da5c` (full SHA in verdict
rows). Raw validation evidence: `g-s5g-validation-20261106`.

Run IDs have prefix `g-s5g-`, the engine/version stem below, and suffix
`-20261106`; retries append `-retry`.

| Engine/version stem | First run | One permitted retry |
|---|---|---|
| mysql-8-0-44 | 19/20; Q18 UPSTREAM_TIMEOUT | Q01 UPSTREAM_TIMEOUT; Q02-Q20 UPSTREAM_ERROR |
| postgres-13-23 | Binding UPSTREAM_TIMEOUT; all 20 not run | 18/20; Q02/Q17 UPSTREAM_TIMEOUT |
| postgres-14-24 | 19/20; Q04 UPSTREAM_TIMEOUT | 18/20; Q15/Q16 UPSTREAM_TIMEOUT |
| oracle-23-5 | 19/20; Q13 UPSTREAM_TIMEOUT | 19/20; Q08 answer-shape mismatch |

Every run exits 1. Each retry was admitted only after a timeout-only first
run and began with fresh fixtures, Builder, self-review, publication and
binding. All eight health gates passed three consecutive one-token calls
within 30 minutes before starting their runs. Later upstream failures show
that this gate does not ensure sustained provider health.

Oracle retry Q08 asks "January exact amount?" Gold is `[["1837.5"]]`.
The Agent queried a month plus SUM, received
`[["2026-01","1837.5000"]]`, and returned both columns. The amount agrees
numerically, but the extra column fails the unchanged exact-row contract.
The owning component is the T6 model Agent's query/answer shape, not a
demonstrated Oracle ingestion error. Its Gateway query succeeded without
truncation. No relaxation or further paid retry was performed.

All eight T5 rows pass only the retained gold-table Builder completeness
check. They are not full-fixture Self-build acceptance. All four source
versions ran natively; Agent DB was PostgreSQL 17. At most two owned database
containers ran simultaneously, all labeled `oo.lane=g`.

## Verification and matrix

Commands and logs are under the validation run:

```sh
cargo check --workspace --locked
cargo build --locked -p oo-builder --bins -p oo-cli -p oo-gateway
CARGO_TARGET_DIR="$PWD/target" cargo build --locked --manifest-path tools/oo-load/Cargo.toml
CARGO_TARGET_DIR="$PWD/target" cargo build --locked --manifest-path tools/release-coordinator/Cargo.toml
python3 evidence/g-s5g-validation-20261106/runs.py
python3 evidence/g-s5g-validation-20261106/verify.py
python3 -m unittest discover -s tools/oo-test -v
```

- Locked check and prerequisite builds pass; 133 oo-test unit tests pass.
- All 4,515 files in eight run manifests verify, including product revision,
  health gates and cleanup. `run-verification.json` records result/manifest
  hashes. Independent final lane-G container and volume inventories are empty.
- `oo-test matrix --merge-only` validates and merges 16 rows into
  `h-merged-82db5207-606a-49bf-92d6-4c61bc5c1941`. Exit 1 preserves the eight
  T6 failures. Exact argv is in `merge-command.json`.
- Evidence index now has 781 rows. Matrix generation, `--check`, `--self-test`
  and `check_spec.py`, `check_plan.py`, `check_v02_plan.py` pass.
- No Rust or Cargo.lock edits; Rust formatting/crate tests are not applicable
  to this Python harness and evidence change. No pre-existing failure waived.

No requested shared-file patch remains. Further paid attempts require new
authorization. Full-fixture T5 and the all-P0 T6 exit remain open; no new T7,
public/business acceptance or source-real-time guarantee is claimed.
