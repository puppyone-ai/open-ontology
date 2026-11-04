# G-S5d E312: single MySQL rerun, 19 correct and one timeout

STATUS: partial. S5-G-01 remains **not done**. The authorized unchanged
MySQL rerun completed once, with 19 correct answers and one upstream model
timeout. This run stands alone; no historical answers are carried forward.

Tested revision: `a32e74a35` (full revision in validation evidence).
No harness, gold, model, budget, timeout or product code was changed.

## Prerequisites and invocation

All three builds exited zero before launching the test:

```sh
cargo build --locked -p oo-builder --bins -p oo-cli -p oo-gateway
CARGO_TARGET_DIR="$PWD/target" cargo build --locked --manifest-path tools/oo-load/Cargo.toml
CARGO_TARGET_DIR="$PWD/target" cargo build --locked --manifest-path tools/release-coordinator/Cargo.toml
tools/oo-test/oo-test t6 --engine mysql --run-id g-s5d-e312-mysql-20261105 --execute
```

The 12 executables (eight Builder binaries, oo, oo-load, oo-gateway and
release-coordinator) were checked for existence and executable permission;
sizes and SHA-256 hashes are retained in `prerequisites.json`.
Lane-G container and volume inventories were empty before launch.

## Result

- Measured source: MySQL **8.4.7**, native linux/arm64.
- Default seed: 2,000,000 events and 200 small tables, confirmed by
  `source/seed.json`; the initial resource receipt describes the smaller
  bootstrap seed, not the later T6 seed.
- Reviewed publisher; 12 gold tables; real Builder 11/21 calls;
  owner review and publication `activated/ready`, with 12 opened fences.
- Native enrollment, login and whoami completed; Agent calls 135/160,
  total model calls 146. Reserved tokens: 5,836,692.
- Command exit **1**; correct 19, failed 1, not run 0.
- Q18 failed on its first model call with `UPSTREAM_TIMEOUT` after
  **300,329 ms**, before any CLI tool call. It returned no usable answer,
  not a wrong or zero answer. Timed-out usage and currency cost are unmeasured.
- Three visible tool rejections: Q14 subquery, Q20 subquery and set
  operation, all `STATEMENT_NOT_ALLOWED`; the Agent recovered using
  supported queries and those questions passed.

| Question | This run |
|---|---|
| Q01 | correct |
| Q02 | correct |
| Q03 | correct |
| Q04 | correct |
| Q05 | correct |
| Q06 | correct |
| Q07 | correct |
| Q08 | correct |
| Q09 | correct |
| Q10 | correct |
| Q11 | correct |
| Q12 | correct |
| Q13 | correct |
| Q14 | correct |
| Q15 | correct |
| Q16 | correct |
| Q17 | correct |
| Q18 | failed: UPSTREAM_TIMEOUT |
| Q19 | correct |
| Q20 | correct |

Every question records `null=false`, `truncated=false`, `refused=false`.
For Q18 these flags describe the absent answer, not a successful result.
No additional invocation, rescore or timeout/budget expansion was attempted.

## Validation and cleanup

Raw run: `evidence/g-s5d-e312-mysql-20261105/`.
Validation: `evidence/g-s5d-e312-validation-20261105/`.

- `cargo check --workspace --locked`: passed.
- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/oo-test -v`:
  69 passed.
- `python3 specs/scripts/check_spec.py`, `check_plan.py`,
  `check_v02_plan.py`: passed.
- All 643 files in the run manifest verified against SHA-256.
- One source and one target, with no concurrent additional database run.
  Both cleanup receipts have `failures: []`; result has `containers_left: 0`.
  Independent final lane-G container and volume scans are empty.
- Pre/post searches found no stale `T6-*.patch` files; none needed deletion.

SHA-256 of the run's `SHA256SUMS`:

```text
67fddd4b691fefc0728e0f18555c14e172bf92b90dde1fe8f3e99bf1ced9f8ac
```

No shared-file patch, Cargo.lock change or Rust edit; Rust formatting is
not applicable. The earlier setup failure and E309 evidence remain intact.
Any further paid rerun needs separate authorization. Full-fixture T5, other
engines/versions, T7 and public/business acceptance were not tested here.
This result does not satisfy the 20/20 or all-P0 exit.
