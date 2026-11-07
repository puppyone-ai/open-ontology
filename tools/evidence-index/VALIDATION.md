# M-S5 validation

Run ID: `m-s5-20261105`. Base: `7bceacc5e`.

`git rebase v0.2/product-loops` reported already up to date. The requested S6
changes were already rebased on arrival: `72e6509f2` -> `e1754fef2`,
`a41aab6f9` -> `4b25daa65`, `b05c806b5` -> `7bceacc5e`. Each pair has an
identical stable patch ID; all three rebased commits remain on this branch.

## S5-M-01 / AT-ENG-03

Done for generated-level/evidence consistency, not P0 engine acceptance.

- `python3 tools/evidence-index/collect.py --report evidence/m-s5-20261105/inventory.json`:
  85 committed files inspected, 18 summaries emit 161 rows. The inventory
  identifies every inspected source and committed-content hash.
- `python3 tools/evidence-index/collect.py --check`: passed, including after
  the collector commit. Output does not depend on dirty files or raw evidence.
- `python3 specs/scripts/gen_support_matrix.py`: passed; generated matrix
  contains 39 exact target/version entries.
- `python3 specs/scripts/gen_support_matrix.py --check` and `--self-test`:
  passed; six derivation cases passed. No hand-maintained support level changed.
- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/evidence-index -v`:
  20 tests passed, covering every adapter, partial/failed outcomes, emulation,
  raw hash precedence, committed-byte fallback, missing verdicts, conflicting
  verdicts, target IDs, patch separation and the generator input contract.
- `cargo check --workspace --locked`: passed before both commits.
- `python3 specs/scripts/check_spec.py`, `check_plan.py`, `check_v02_plan.py`:
  passed before both commits; `python3 execution/v0.2/render_todo.py --check` passed.
- No Rust crate changed: crate tests and cargo fmt are not applicable.
- Staged secret/path/size scans and `git diff --cached --check`: passed.

The first 17-test invocation had one test-fixture replacement error: replacing
`| passed |` missed alternating cells because separators overlap. Replacing
the value itself fixes that test input; the collector and expected empty
result were not weakened. Original failed output remains in `tests-1.log`.

## Results and boundaries

`REPORT.md` contains the complete target x exact version x capability table,
and every missing P0 T0-T8 suite, including all historical patches and 19c.
Only SQL Server 2019/2022/2025 patches reach the generator's narrow `full`
gate (T0/T1/T7/T8, amd64 emulated); CDC/digest remain experimental. No P0
full capability is verified, and no schema_change/selfbuild/dialogue level
is emitted. Missing evidence is distinguished from only skip/fail evidence.

Some successful historical component work intentionally produces no product
pass row: summaries that omit explicit suite verdicts must be improved by
their owners. Cycle counts, Builder metrics, successful commands and audit
smoke do not substitute for missing acceptance. The generator's existing
lexicographic run-ID selection remains unchanged and is not asserted to be
chronological. Raw database suites were not rerun or independently rehashed.

Shared-file patch requests: none. Cargo.lock changes: none. No customer
systems, services, other worktrees, tokens, images or containers were touched.
Containers created or left running by this card: none.
