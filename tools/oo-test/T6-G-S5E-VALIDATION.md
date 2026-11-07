# G-S5e: remaining P0 T5/T6 measurements

S5-G-01 is **not complete**: only MySQL 5.7.44, Oracle 21.3 and Oracle 11.2 passed T6 among the ten versions on this card. All ten were exercised sequentially. Failures and prior interrupted evidence are retained; answers were never combined.

## Execution and scope

- Rebased onto `v0.2/product-loops` at `14dc87974` before resuming. Resumed runs tested `5cf2d117df78fc28368eef227693357d8b203641` with a clean tracked tree.
- Recovered runs do not contain a product SHA in their verdict rows; none was invented. Their raw hashes were validated before merging.
- T5 passes only describe completeness of the 12 retained gold-question tables and their columns, including owner self-review. They are **not full-fixture T5 acceptance**.
- The historical MySQL 5.7.44 T5 row lacked the later scope metadata. The committed index annotates its scope from the existing raw `result.json` T5 scope; raw files, verdicts, hashes and timestamps remain unchanged.
- Same model references and budgets; default 2,000,000-event scale; 300-second model egress timeout. One independent retry only after a timeout-only failure. The interrupted PostgreSQL 16.15 run was restarted as authorized by the resume card.
- At most two owned database containers simultaneously, plus the Oracle 11g thick worker where needed. All cleanup receipts report zero remaining containers; final lane-label inventory is empty. No other containers were stopped or removed.

## Results

All run IDs below have prefix `g-s5e-`. Every run has a passing scoped T5 verdict. `not_run` is not a wrong answer or a zero.

| Run suffix | T6 correct / 20 | Remaining outcomes |
|---|---:|---|
| `mysql-5-7-44-20261105` | 20 | pass |
| `mysql-8-0-44-20261105` | 19 | Q05: UPSTREAM_TIMEOUT |
| `mysql-8-0-44-20261105-retry` | 18 | Q02: UPSTREAM_TIMEOUT, Q20: UPSTREAM_TIMEOUT |
| `mysql-9-5-0-20261105` | 0 | Q01: not_run, Q02: not_run, Q03: not_run, Q04: not_run, Q05: not_run, Q06: not_run, Q07: not_run, Q08: not_run, Q09: not_run, Q10: not_run, Q11: not_run, Q12: not_run, Q13: not_run, Q14: not_run, Q15: not_run, Q16: not_run, Q17: not_run, Q18: not_run, Q19: not_run, Q20: not_run; UPSTREAM_TIMEOUT |
| `mysql-9-5-0-20261105-retry` | 18 | Q18: UPSTREAM_TIMEOUT, Q20: mismatch |
| `oracle-11-2-20261105` | 20 | pass |
| `oracle-21-3-20261105` | 20 | pass |
| `postgres-13-23-20261105` | 18 | Q08: UPSTREAM_TIMEOUT, Q13: UPSTREAM_TIMEOUT |
| `postgres-13-23-20261105-retry` | 0 | Q01: not_run, Q02: not_run, Q03: not_run, Q04: not_run, Q05: not_run, Q06: not_run, Q07: not_run, Q08: not_run, Q09: not_run, Q10: not_run, Q11: not_run, Q12: not_run, Q13: not_run, Q14: not_run, Q15: not_run, Q16: not_run, Q17: not_run, Q18: not_run, Q19: not_run, Q20: not_run; UPSTREAM_TIMEOUT |
| `postgres-14-24-20261105` | 19 | Q02: UPSTREAM_TIMEOUT |
| `postgres-14-24-20261105-retry` | 18 | Q14: UPSTREAM_TIMEOUT, Q19: UPSTREAM_TIMEOUT |
| `postgres-15-19-20261105` | 18 | Q15: UPSTREAM_PROTOCOL, Q20: UPSTREAM_PROTOCOL |
| `postgres-16-15-20261105` | 14 | Q15: not_run, Q16: not_run, Q17: not_run, Q18: not_run, Q19: not_run, Q20: not_run; interrupted by session restart |
| `postgres-16-15-20261105-restart` | 16 | Q05: UPSTREAM_ERROR, Q06: UPSTREAM_ERROR, Q07: UPSTREAM_ERROR, Q08: UPSTREAM_ERROR |
| `postgres-18-6-20261105` | 0 | Q01: not_run, Q02: not_run, Q03: not_run, Q04: not_run, Q05: not_run, Q06: not_run, Q07: not_run, Q08: not_run, Q09: not_run, Q10: not_run, Q11: not_run, Q12: not_run, Q13: not_run, Q14: not_run, Q15: not_run, Q16: not_run, Q17: not_run, Q18: not_run, Q19: not_run, Q20: not_run; UPSTREAM_TIMEOUT |
| `postgres-18-6-20261105-retry` | 7 | Q08: UPSTREAM_ERROR, Q09: UPSTREAM_ERROR, Q10: UPSTREAM_ERROR, Q11: UPSTREAM_ERROR, Q12: UPSTREAM_ERROR, Q13: UPSTREAM_ERROR, Q14: UPSTREAM_ERROR, Q15: UPSTREAM_ERROR, Q16: UPSTREAM_ERROR, Q17: UPSTREAM_ERROR, Q18: UPSTREAM_ERROR, Q19: UPSTREAM_ERROR, Q20: UPSTREAM_ERROR |

MySQL 9.5.0 retry Q20 returned `[["1"],["1"]]`, not gold `[["2"]]`. No gold value or test was weakened. PostgreSQL 15.19 protocol errors and restarted 16.15 upstream errors did not qualify for the timeout-only retry. MySQL 8.0.44 and PostgreSQL 13.23/14.24 exhausted their authorized retries before resumption.

## Evidence and validation

- Raw validation run: `g-s5e-resume-20261105`; original implementation validation: `g-s5e-validation-20261105`.
- `oo-test matrix --merge-only` with all 16 evidence directories: hash/schema validation succeeded, emitted 32 rows to `h-merged-b5f21440-62a9-46af-8523-c7ed955ee548`; exit 1 accurately reports failed T6 rows. Exact argv and stdout are in the resume evidence.
- `cargo check --workspace --locked`: pass. Prerequisite builds for oo-cli, oo-builder, oo-gateway, tools/oo-load and tools/release-coordinator: pass.
- `python3 -m unittest discover -s tools/oo-test -v`: 124 pass.
- `python3 -m unittest discover -s tools/pg-builder/scripts -p test_retained_build.py -v`: 8 pass.
- `cargo test --locked -p oo-builder`: pass.
- `python3 specs/scripts/gen_support_matrix.py`, `--check`, `--self-test`: pass (698 evidence rows). These are generator consistency checks, not product acceptance.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: pass. No Rust edits, so formatting is not applicable. No Cargo.lock changes.

## Required coordinator follow-up

`G-S5E-MATRIX-SCOPE.patch` is the exact proposed change to coordinator-owned `specs/scripts/gen_support_matrix.py`; it was not applied. The current generator ignores the limited scope and renders ten selfbuild cells as verified. **Do not accept those cells as full-fixture coverage.** The patch keeps limited T5 cells experimental without changing the raw pass verdict; its isolated validation passed the existing self-test and checked all ten scoped versions. Regenerate the matrix after applying it.

Further model retries beyond this card are not authorized. Full-fixture T5, failed T6 versions, and the earlier MySQL 8.4.7 19/20 result still prevent the all-P0 exit. Earlier PostgreSQL 17.11 / Oracle 23.5 / MySQL 8.4.7 runs were not rerun or upgraded by this card.
