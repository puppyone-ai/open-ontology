# M-S2b author validation

All four card TODOs meet their requested T2 exits. This is compact, synthetic
author evidence, not full T0-T8 verification, T7 audit, Gateway publication,
large-scale/type coverage, or independent business acceptance.

Product code tested: `e5c7fc6bba2cd6448be9832ed2ad163c3bb2795e`.

## Engine matrix

| Engine/version | Platform | C1 | C5 | Maximum sampled duty | Peak reader sessions |
|---|---|---|---|---|---|
| mysql `mysql:5.7.44` | amd64 | 3/3 | 3/3 | 0.00% | 2 |
| mysql `mysql:8.0.44` | arm64 | 3/3 | 3/3 | 0.00% | 2 |
| mysql `mysql:8.4.7` | arm64 | 3/3 | 3/3 | 0.00% | 2 |
| mysql `mysql:9.5.0` | arm64 | 3/3 | 3/3 | 0.00% | 2 |
| postgres `postgres:13.23-bookworm` | arm64 | 3/3 | 3/3 | 6.94% | 2 |
| postgres `postgres:14.24-bookworm` | arm64 | 3/3 | 3/3 | 2.58% | 2 |
| postgres `postgres:15.19-bookworm` | arm64 | 3/3 | 3/3 | 7.73% | 2 |
| postgres `postgres:16.15-bookworm` | arm64 | 3/3 | 3/3 | 4.69% | 2 |
| postgres `postgres:17.11-bookworm` | arm64 | 3/3 | 3/3 | 3.50% | 2 |
| postgres `postgres:18.6-bookworm` | arm64 | 3/3 | 3/3 | 3.41% | 2 |
| oracle `docker.io/gvenzl/oracle-free:23.5-slim` | arm64 | 3/3 | 3/3 | 15.18% | 2 |
| oracle `docker.io/gvenzl/oracle-xe:21.3.0-slim` | amd64 | not required/run | 3/3 | 15.13% | 1 |
| oracle `docker.io/gvenzl/oracle-xe:11.2.0.2` | amd64 | not required/run | 3/3 | 10.29% | 1 |

MySQL is built with `--features mysql-binlog`; default artifacts still exclude C1.
Oracle 11 uses the labelled amd64 thick worker. Oracle 21/11 exercise default
C5 and refusal of implicit LogMiner; Oracle 23 additionally exercises opt-in C1.
Oracle C1 T8 counts both the data reader and common-user miner sessions.

The lane was rebased onto `5bba3e453` before editing. H's aligned pins were not
present in that checkout. MySQL explicitly uses 5.7.44/8.0.44/8.4.7/9.5.0;
PostgreSQL explicitly uses 13.23/14.24/15.19/16.15/17.11/18.6-bookworm. Tags
were resolved to installed RepoDigests and pulled by digest; each resource
receipt records the actual image/platform. No shared pin file was changed.

## TODO exits and commands

- S2-C-01: done for this card. Three file-position C1 cycles on all four
  versions, numeric monotonic positions, inserts/updates/deletes/key changes,
  persistent epoch rejection and exact target equality. Zero no-change
  logical writes/generations and unchanged physical target relation bytes.
- S2-C-02: done for this card. Three durable four-layer C5 cycles per version;
  hints never exclude ranges and periodic reads remain enabled. Handoff covers
  two distinct source databases on one physical MySQL fixture, two bindings
  each, concurrent writes, L0-to-end repair, target-atomic switch, and C1
  continuation. A missing second baseline rolls back the first publication
  and cursor. Both sources have C5 checkpoints before the successful handoff.
- S2-D-01: done for this card. Six aligned PostgreSQL versions each pass three
  C1 and three C5 cycles, exact equality, no-change checks, T8, expired-slot
  refusal without target mutation, and recreated-relation refusal.
- S2-E-01: done for this card. Oracle 23 C1+C5 and 21/11 C5 each pass three
  cycles including no-change. A transaction starts before the Oracle 23
  baseline, is proven absent from baseline rows, commits afterward, and is
  recovered by the restarted miner. Changing the persisted floor is rejected
  without target mutation. DBID/resetlogs and the fixed floor bind the cursor.

Commands:

```sh
python3 tools/oo-test/t2_p0.py --engine mysql --versions 5.7,8.0,8.4,9 --run-id m-s2b-mysql-distinct-20261104
python3 tools/oo-test/t2_p0.py --engine oracle --versions 23,21,11 --worker-image oo-e-oracle-worker:e-fix-worker-v4 --run-id m-s2b-oracle-observed-20261104
python3 evidence/m-s2b-build-20261104/aligned_pg_final.py
python3 tools/oo-load/test-sync-target.py --run-id m-s2b-target-regression-20261104
```

The PostgreSQL wrapper invokes `tools/oo-test/t2.py` with all six majors and
records its explicit image map in `aligned-pg-final-images.json`. All matrices
write raw commands, comparisons, budgets, failures, cleanup and SHA256SUMS.

## Regression checks

- `cargo check --workspace --locked`: passed before each commit.
- `cargo test --manifest-path tools/oo-load/Cargo.toml --locked`: 9 passed,
  6 ignored; with `--features mysql-binlog`: 10 passed, 6 ignored.
- Connector unit tests with `mysql-binlog,oracle`: 237 passed, 5 ignored.
- Full connector run: one pre-existing macOS failure,
  `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`
  (`group_signal` rejected rather than sent). The exact test fails identically
  on archived base `5bba3e453`; see `base-proof.json`, `connectors-full.log`,
  and `base-parent-deadline.log` in `m-s2b-build-20261104`. All other connector
  tests pass when only that proven base failure is excluded. No test was changed.
- `python3 -m unittest discover -s tools/oo-test -p 'test_*.py'`: 38 passed.
- Real target regression: atomic overlay/resume, C5 parent reuse, zero-write
  no-change, and actual SIGKILL after commit all passed; cleanup empty.
- `git apply --check tools/oo-load/SHARED-DOCS.patch`: passed; patch not applied.

## Engine changes, shared files, and limits

- `mysql/engine.rs`: expose a live source epoch and allow explicit file cursors
  on GTID-enabled servers without altering server settings. Unit test retains
  disabled-binlog refusal.
- `oracle/engine.rs`: expose live DBID/resetlogs for C5 and pre-commit checks.
- `oracle/logminer.rs`: admit restart only with a persisted pre-baseline
  transaction floor covered by retained redo and active-transaction bounds.
  Guard unit tests cover purged history, future SCNs and invalid floors.
  Existing connector empty-window semantics remain unchanged; only the
  loader suppresses empty Oracle windows as target data generations.
- Root Cargo.toml/Cargo.lock, existing migrations, shared engine interface,
  docs/specs/execution and support matrix are unchanged. Standalone
  `tools/oo-load/Cargo.lock` adds optional binlog dependencies already pinned
  in the workspace, including zstd 0.13.4 and zstd-safe 7.4.0.
- Exact coordinator-owned playbook patch: `SHARED-DOCS.patch` (unapplied).
- C1 baseline alignment remains caller-asserted outside MySQL repair handoff.
  Oracle must retain the fixed transaction floor for the whole baseline
  lifetime; it fails closed on purge rather than advancing the floor.
- Handoff is bounded: at most 16 sources, 100000 keys/32 MiB per source, and
  each final table must fit the chunk budget. This is not disk-spooled
  unbounded bootstrap. Distinct source databases share one physical fixture;
  independent-server failover is not claimed. Physical C5 plans and changed
  range plans still require explicit rebaseline.
- Fixtures use keyed numeric/text data (MySQL UTF-8); no full type/scale
  coverage or new T7 audit is claimed. T8 is sampled, not continuous tracing.

## Preserved unsuccessful runs

- Earlier Oracle C1 T8 reports (including implementation commit descriptions)
  used PDB-local V$SESSION and missed the root miner. Those resource claims
  are superseded, not accepted as full C1 T8 evidence. The final
  `m-s2b-oracle-observed-20261104` matrix samples from CDB$ROOT, observes both
  sessions, and passes the unchanged two-session/25-percent budgets. A unit
  regression enforces the root-container query and both account filters.
- `m-s2b-mysql84-probe-20261104`: short idle C1 poll sampled 76.75% duty.
- `m-s2b-mysql-final-20261104`: 5.7 default Latin-1 rejected by the existing
  binlog charset guard; fixture explicitly made UTF-8 for parity. 8.4 small
  changed C5 cycle sampled 26.61% duty, fixed by conservative pacing. 9.5
  handoff exposed default GTID versus durable file-position mismatch, fixed
  by explicit client cursor selection, not changing source GTID settings.
- `m-s2b-oracle-20261104`: C5 passed; 23 C1 refused a prior-session SCN.
  Fixed via persisted transaction-floor admission, not deleting the guard.
- `m-s2b-oracle23-resume-20261104`: fixture comparison used integer equality
  on canonical Oracle text keys; changed test SQL to exact numeric casts.
  No gold values were changed. `...resume-b...` passed, followed by the final
  accepted matrix after moving empty-window suppression into the loader.

All created containers/workers/volumes were removed; final cleanup receipts
are empty. No customer systems, unrelated resources, other worktrees, pushes
or PRs were used.

## Evidence hashes

- `evidence/m-s2b-mysql-distinct-20261104/matrix.json`: `2d691676a75dc65148c3cfec7bad8ed46edee1be1d70d94bbf8325f9fe71eea1`
- `evidence/m-s2b-pg-final-20261104/matrix.json`: `a343604ecf408345a8a0d6b8697d71a6255baa521b392bed5d49cc7a7f4f5332`
- `evidence/m-s2b-oracle-observed-20261104/matrix.json`: `a1eb493ff4adfb47f2618438ea9fa5e59e9115137c3e51883910b7bbc2afa300`
- `evidence/m-s2b-target-regression-20261104/result.json`: `7f07379cef9807a2c955595c52d952324a8cabd496c75f94890f8837d9001dad`
