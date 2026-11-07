# H-S5c: concurrent snapshot verdict, blocked scale exit

2026-11-05. S5-H-01 remains open. The M1 harness fix is complete; the
13-version full-scale acceptance is blocked by the product's PostgreSQL
no-key C5 path. No fixture exclusion or full-reload fallback was introduced.

## Code and regression

The existing executor commit was retained and rebased onto `14dc87974`.
Rebased commit `dde1e0d78` replaces the quiescent full reload with one
`oo-load sync --c5` correction on the concurrent snapshot's relations.
The replay remains a no-write check, not the verdict. Correction errors,
non-C5 receipts, or changed table/target bindings fail closed.

The negative comparator test rejects a dropped row, a duplicated row, and
count-preserving drop-plus-duplication. These are synthetic loaded-chunk
test doubles, not live database fault injection.
The orchestration regression fails on the rebased base `14dc87974`
(both success/error subcases) and passes on this lane.

## Runtime evidence

Retained run `h-s5c-postgres-20261105` seeded 2,000,000 events and 200 small
tables for every attempted version. Original failures were not overwritten:

| Version | T1 | T8 | Evidence |
| --- | --- | --- | --- |
| PostgreSQL 13.23 | fail | partial | C5 `stale_chunk` |
| PostgreSQL 14.24 | fail | partial | C5 `ctid_requires_exported_snapshot` |
| PostgreSQL 15.19 | fail | partial | C5 `ctid_requires_exported_snapshot` |
| PostgreSQL 16.15 | fail | partial | C5 `ctid_requires_exported_snapshot` |
| PostgreSQL 17.11 | interrupted/fail | not run | interrupted executor |
| PostgreSQL 18.6 | not run | not run | no evidence |
| MySQL 5.7.44, 8.0.44, 8.4.7, 9.5.0 | not run | not run | no card rerun |
| Oracle 11.2.0.2.0, 21.3.0.0.0, 23.5.0.24.07 | not run | not run | no card rerun |

The prior full-scale failures predate this rebase. They are not represented
as tests of the rebased product. Fresh reduced diagnostics on `dde1e0d78`:

```sh
OO_TEST_LANE=h PYTHONDONTWRITEBYTECODE=1 python3 tools/oo-test/t1.py \
  --engine postgres --versions 13,14 --scale 1000 --tables 3 \
  --run-id h-s5c-resume-pg-diagnostic-20261105
```

Exit 1. PostgreSQL 13 passes T1/T8 (36 tables); PostgreSQL 14 fails T1,
T8 partial. All owned resources were cleaned. The 14 run emits 18 C5
receipts before `public.no_key` fails with
`source error: protocol fact=ctid_requires_exported_snapshot` in
`postgres-14/0031.stderr`. These diagnostic rows are NOT merged into
full-scale acceptance evidence.

The connector's `plan_inner` requires an exported snapshot to plan a
no-key heap as ctid ranges. C5 calls it without that snapshot. Merely opening
a snapshot is insufficient: `tools/oo-load/src/c5.rs::ordered_chunks`
explicitly rejects physical ranges as non-durable, and `same_ranges` checks
the prior snapshot plan. The product owner must define a safe no-key
snapshot-to-C5 transition without silently replacing the whole verdict
with a reload. This cannot be repaired solely in the card's harness scope.
The old suggested SingleRead patch is obsolete: the rebased connector
already admits unchanged SingleRead plans, confirmed by the PG13 diagnostic.

## Merge and shared-file handoff

`evidence/h-s5c-resume-20261105/regenerate.py` runs the real merge-only
command on the retained full-scale directory, then the unchanged matrix
generator with only its index/output locations redirected into evidence.

- Merge: `h-merged-94218dcf-ab6e-4f7f-a20f-74d2e3286673`, 10 explicit failure
  rows, exit 1 as required for failed suites.
- Candidate index: 676 total rows. Generation and `--check --self-test`
  pass, including all nine derivation cases.
- Exact coordinator-owned index/matrix diff:
  `evidence/h-s5c-resume-20261105/shared-matrix.patch`.
  `git apply --check` passes. SHA256:
  `2b5a93c2658305821ed010fd03540460aa2595efcd7e38f261f6db97bfd58a36`.
- No shared index, matrix, engine, audit/T7, t6, or Cargo.lock was changed.
  No speculative product patch is proposed as a verified fix.

## Validation and remaining work

Raw outputs: `evidence/h-s5c-resume-20261105/`.

- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo check --workspace --locked`:
  pass.
- `CARGO_TARGET_DIR=target cargo build --locked --manifest-path tools/oo-load/Cargo.toml`
  with the same profile variables: pass.
- `python3 -m unittest discover -s tools/oo-test -p 'test_*.py'`: 123 pass.
- `cargo test --locked --manifest-path tools/oo-load/Cargo.toml --lib`:
  4 pass, 2 ignored; ignored tests are not passes.
- The full `cargo test --locked --manifest-path tools/oo-load/Cargo.toml`
  also passes: 39 passed, 7 ignored across its test binaries.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: pass.
- No Rust files changed; crate formatting is not applicable.
- No pre-existing-failure exemption is needed for these commits.

Follow-up: product owner fixes the PostgreSQL no-key C5 transition; rerun
all 13 versions at full scale, merge their explicit rows, and regenerate
the shared matrix through coordinator review. This card does not establish
T7, T6, public Gateway, or business acceptance. Containers left running by
this resumed executor: none.
