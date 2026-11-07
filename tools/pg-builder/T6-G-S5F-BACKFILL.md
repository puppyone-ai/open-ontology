# G-S5f: hash-verified historical verdicts

S5-G-01 remains incomplete. These are historical verdicts, not new model runs.
Rebased onto `50b625c71` before doing this work. No product code changed.

The three local archives were extracted inside this worktree. Each original
SHA256SUMS matched its previously committed digest in the E309/E312 validation
report, and every listed file verified. The original manifests and raw results
remain unchanged. Derived reports live in `evidence/g-s5f-backfill-20261105/`,
with the original run IDs, result/manifest hashes, measured versions, original
result timestamps and historically tested product revisions.

| Original run ID | Files verified | Answers | T5 row | T6 row |
|---|---:|---:|---|---|
| g-s5c-e309-pg-20261105 | 565 | 20/20 | pass, limited scope | pass |
| g-s5c-e309-oracle-20261105 | 661 | 20/20 | fail | fail |
| g-s5d-e312-mysql-20261105 | 643 | 19/20 | pass, limited scope | fail |

Oracle's original result says `status=failed`, `cleanup_error=RuntimeError`
and `containers_left=unmeasured`. The existing verdict policy fails both rows
when cleanup is not proved. Twenty correct answers do not erase that failure.
MySQL's missing answer is an upstream timeout, not an invented zero.

Builder completeness was recomputed with the existing
`t6_reviewed.completeness` against the verified facts, draft and owner-reviewed
registry. All three cover only retained gold-question tables, not full-fixture
T5. The scope field prevents the generator from claiming full Self-build
verification. The rows were emitted with `verdicts.write_rows`; historical
provenance replaces the function's default current timestamp/revision.

Validation:

- `tools/oo-test/oo-test matrix --merge-only --merge-evidence evidence/g-s5f-backfill-20261105`:
  six hash-validated rows; exit 1 intentionally preserves explicit failures.
- `python3 specs/scripts/gen_support_matrix.py`, `--check`, `--self-test`: pass.
- `cargo check --workspace --locked`: pass.
- `python3 -m unittest discover -s tools/oo-test -v`: 131 tests pass.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: pass.

Validation run ID: `g-s5f-validation-20261105`; archive receipt:
`archive-verification.json`. The merge package is
`h-merged-d8b9149a-6be1-49f1-9abf-550164705d02`.

No Cargo.lock change, Rust edit, source fix or new database run is claimed by
this backfill. Current reruns and the Q20 diagnosis are reported separately.
