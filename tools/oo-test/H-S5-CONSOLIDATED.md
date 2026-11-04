# H-S5 consolidated attempt, 2026-11-05 (Asia/Shanghai)

Status: partial. Neither S5-H-01 nor S5-H-02 closes.

Rebased onto `v0.2/product-loops`. Runner implementation: `77c39378e`.
Build/unit evidence: `h-s5-consolidated-20261105`.
Final matrix run: `h-s5-20261104T223052426255Z-78add835-213e-4d50-acb2-4dab567c0f61`
(UTC timestamp, the same local calendar day).

## Attempt and explicit verdicts

```sh
OO_TEST_LANE=h tools/oo-test/oo-test matrix --all --suites T0-T4,T7,T8
python3 specs/scripts/gen_support_matrix.py
python3 specs/scripts/gen_support_matrix.py --check --self-test
```

Matrix exit 1. All 14 targets were visited sequentially, with no containers
started. Thirteen locked targets failed the mandatory 20 GiB disk gate:
21,144,936,448 to 21,145,174,016 free bytes, below 21,474,836,480.
Oracle 19c failed resolution of its licensed image (E014).
These are infrastructure-blocked attempts, NOT executed database test failures.
All 140 explicit rows are `fail`; no pass is inferred. `version_measured=false`
and `emulated=false` mean no server or emulated execution was observed.

| Engine | Locked version identifiers | Verdict and generated capabilities |
|---|---|---|
| MySQL | 5.7.44, 8.0.44, 8.4.7, 9.5.0 | Each disk-blocked; full/cdc/digest/schema_change designed |
| PostgreSQL | 13.23, 14.24, 15.19, 16.15, 17.11, 18.6 | Each disk-blocked; same four levels designed |
| Oracle | 23.5, 21.3.0, 11.2.0.2 | Each disk-blocked; same four levels designed |
| Oracle | 19c | E014; same four levels designed |

Oracle identifiers above are image versions, not measured database patch
versions. No evidence transfers to 23.5.0.24.07, 21.3.0.0.0 or 11.2.0.2.0.
T5/T6 were not requested; selfbuild/dialogue capabilities are absent.
The integration base had no evidence-index file; historical lane-M collector
rows were not copied or reinterpreted. No previous capability was promoted.
The generator and its seven derivation self-tests pass for all 140 rows.

## Implementation checks

- `cargo check --workspace --locked`: pass.
- `cargo build --manifest-path tools/oo-load/Cargo.toml --locked --features mysql-binlog`: pass.
- `python3 -m unittest discover -s tools/oo-test -p 'test_*.py'`: 78 pass.
- `python3 -m unittest discover -s ci -p 'test_*.py'`: 39 pass.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: pass.
- No Rust crate edits; crate formatting/tests not applicable.
- Raw results are SHA-256 checked before merging. New tests cover tampering,
  unmeasured versions, cleanup failure, partial fault suites, meter coverage,
  missing verdicts, path escape and non-UTC timestamps.
- No shared patch requested; no Cargo.lock change.

## Remaining exits

Restore at least 20 GiB of free disk plus runtime headroom outside this card,
then rerun real workloads; this executor did not delete unrelated data or
build prerequisites to evade the gate. Standalone rows can be merged via
`--merge-evidence`, but end-to-end built-in orchestration of all P0 suites
is not complete: product doctor and incremental/fault hooks still need
wiring. A bare matrix without hooks fails rather than treating fixture T0
or hook exit codes as product acceptance. T7 must cover the actual T1/T2
reader sessions; merging independent fixtures is not an audit proof.
Oracle 11g requires its thick-worker prerequisite. Three consecutive stable
nights remain unproved. No T5/T6/model runs or scheduling changes were made.

No containers or volumes were created by this attempt; the post-run lane-H
container and volume queries were empty. Other services were not modified.
