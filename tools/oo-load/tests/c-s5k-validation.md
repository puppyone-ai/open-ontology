# C-S5k: whole-table keyless C5

Status: partial. Rebased onto `b96ce601c86d287cef2977dd4228aee7ae5be063`.
The product correction is implemented; the required Oracle 23 T1 exit is
blocked by an independently reproduced, pre-existing keyed LOB digest error.
No fixtures, gold values, or failed evidence were removed.

## Behavior

- A keyless table has one unbounded C5 source digest. Physical chunks remain
  private to their snapshot/generation; they are never adopted from a parent.
- Changed tables replace every part in the existing atomic generation
  transaction. Partial, duplicate, wrong-revision, wrong-scope, wrong-row-count,
  changed-key, and stale-parent replacements remain rejected.
- PostgreSQL/Kingbase C5 keeps an exported snapshot alive. Releasing the cached
  reader before lineage checks keeps exporter plus reader/control within two
  connections. Oracle unbounded keyless digests use the plan's SCN, including
  SingleRead plans.
- Established idle cycles stage no payload and create no generation. Periodic
  full reads also avoid writes when unchanged. A separate order-independent,
  duplicate-sensitive payload checksum catches source-digest collisions without
  depending on physical boundaries, row order, or IPC plan metadata.
- An old snapshot without whole-table evidence is conservatively refreshed on
  its first C5 cycle. Existing SingleRead row/byte limits remain enforced.
- SQL Server, MariaDB, TiDB, OceanBase, Db2 and Kingbase use the shared keyless
  path; explicit C5 routing now admits the already implemented family readers.
  There is no DM8 reader/dispatch implementation in this checkout to adapt.

## Live corrected-judge evidence

All runs use `OO_TEST_LANE=c PYTHONDONTWRITEBYTECODE=1 python3
tools/oo-test/t1.py --engine ENGINE --versions VERSIONS --scale 1000 --tables 3
--run-id RUN`. These are reduced-scale regressions, not full-scale acceptance.
The writer now mutates `no_key` as well as `growth_events`, including duplicate
inserts, updates and deletes. Correction still uses exactly one product C5
cycle, followed by source/target comparison. Subsequent keyless-only idle and
periodic full-read probes assert zero writes, payload and new generations.

| Run | Actual version | T1 | T8 | Compared tables |
| --- | --- | --- | --- | --- |
| `c-s5k-pg-final-20261105` | PostgreSQL 13.23 | pass | pass | 36 |
| same | PostgreSQL 14.24 | pass | pass | 36 |
| same | PostgreSQL 17.11 | pass | pass | 36 |
| `c-s5k-mysql-final-20261105` | MySQL 8.4.7 | pass | pass | 34 |
| `c-s5k-oracle-final-20261105` | Oracle 23.5.0.24.07 | fail | partial | 0 |
| `c-s5k-oracle-base-20261105` | Oracle 23.5.0.24.07, base product | fail | partial | 0 |

Both Oracle runs fail during correction of keyed `TYPE_EDGES`, before
`NO_KEY`, with `catalog fact=lob_source_digest_requires_streaming`:
`oracle-23/0030.stderr` (patched) and `oracle-23/0028.stderr` (base).
The base binary was built from a `git archive` of the rebased base inside
the ignored evidence directory and passed explicitly via `--binary`.
Implementing general Oracle LOB source digests is outside this keyless card.

Initial diagnostic run `c-s5k-pg-20261105` is retained: all three T1 comparisons
passed, but PG13/17 T8 detected a third source connection. The final run fixes
that problem; the original failures are not overwritten.

SHA256 of each final `ENGINE-VERSION/t1-t8.json`, in table order:

```text
371e1f1423ee4d74ebe2dc706851f604fdbe39abdbe0ec446341d8bab3be11d7
39d2a79a1dddc68029dd4b0ae6e9f192115000e7eb37fb416244c83148bda46a
9f08812c50151163bacedb399ad4bc7da58b78ad1f718bc194deb793c38f0436
90afc905c6cf778637267e91fc082473f2c9a19914bde32ef625632cf47a73f2
7078027ec9260880635ca0ba28a1384f4085e9727b804f9721053322f4c22fd4
572c642191e4a76224f8c3b447af8d63824932ca2e568a5c54048e61ecbaacb5
```

Each report records its actual binary SHA256. Two build profiles of the same
production patch were exercised; these are not claimed as one binary pin.

## Local validation

Raw logs are in `evidence/c-s5k-20261105/`.

- `cargo check --workspace --locked`: pass.
- `cargo test --locked -p oo-pipeline`: 355 passed, 55 ignored.
- `cargo test --locked -p oo-connectors --features oracle,mysql,sqlserver,db2`:
  one host failure, `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`.
  The exact test fails identically on archived base `b96ce601c`
  (`base-parse-direct.log`): expected process-group signal `sent`, got `rejected`.
  Rerun with only that proven failure filtered: 402 passed, 40 ignored.
  The eight `file_worker_parent` tests pass on this checkout.
- `cargo test --locked --manifest-path tools/oo-load/Cargo.toml`:
  40 passed, 7 ignored. Shared driver doubles cover all nine implemented
  keyless engine families: initial correction, idle, row reorder, full-read
  digest collision, duplicates and empty-table replacement. These are not
  claims of live engine/version coverage.
- `OO_LOAD_TEST_TARGET=<owned config> cargo test --locked --manifest-path
  tools/oo-load/Cargo.toml --test sync_target
  c5_target_reuses_parent_and_no_change_writes_nothing -- --ignored --exact`:
  one live target test passed on this run's own target.
- `python3 -m unittest discover -s tools/oo-test -p 'test_*.py'`:
  123 passed; the negative dropped/duplicated-row judge tests remain intact.
- Touched Rust crates were formatted; no lockfile, migration, shared
  engine interface, specification, support matrix or execution file changed.

## Exit and handoff

S5-H-01, S5-C-04, S5-D-03, S5-E-04 remain open at plan level: this card supplies
the C5 defect correction and reduced-scale evidence, not full T7/T3/T4 or the
complete version matrix. The Oracle required live exit is not satisfied.
Coordinator follow-up: resolve keyed Oracle LOB digests, rerun the unmodified
full fixture, then integrate evidence/matrix changes in the shared write scope.
All owned containers were removed; no other lane's resources were modified.
