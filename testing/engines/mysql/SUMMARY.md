# C-S0S1 executor sign-off (2026-11-04)

Status: partial. Implementation and four-version adapter tests are delivered
in the worktree, uncommitted. The full-crate test gate fails in unchanged,
out-of-scope file-worker code; no gate was skipped to make a commit.

## TODO exits

- S0-C-01: done at adapter level. All four real T0 probes, missing SELECT
  and replication grants, version dispatch, GTID/file-position watermarks,
  and real tagged GTIDs on 8.4/9.5 pass.
- S1-C-01: implementation and adapter T1 harness pass; full AT-ENG-01 is
  not done. Continuous production CDC, Agent DB integration and three-cycle
  T2 acceptance belong to S2/coordinator work, not this harness.
- S1-C-02: source-side T8 passes: one observed reader connection, measured
  read duty below 25%, load pauses, server/client statement bounds and
  session network timeout setup. Timeout-disconnect fault injection and
  target/model resource accounting are not established by these tests.

## Final live results

Command: `python3 testing/engines/mysql/run.py --run-id RUN_ID`
(live09 used `--versions 5.7.44 8.0.44`).

| Exact version | Run ID | T0 | Adapter T1 | Peak reader connections | Read duty |
| --- | --- | --- | --- | --- | --- |
| 5.7.44, amd64 | c-s0s1-20261104-live09 | pass | zero diff | 1 | 7.557% |
| 8.0.44, arm64 | c-s0s1-20261104-live09 | pass | zero diff | 1 | 6.200% |
| 8.4.7, arm64 | c-s0s1-20261104-live08 | pass | zero diff | 1 | 8.900% |
| 9.5.0, arm64 | c-s0s1-20261104-live08 | pass | zero diff | 1 | 8.075% |

Each version checks a static 2,000,000-row fixture and all key shapes.
The concurrent writer test covers 2,000 rows, 100 committed transactions,
500 affected old/new keys, and 20 source-compared ranges. It uses a
checksum-verified real binlog window and a verified quiescent end cut;
it is not a production streaming CDC test or an Agent DB acceptance run.

Source hashes in these four evidence directories match the delivered Rust
and runner files. Per-artifact hashes, including binary log windows:
`evidence/c-s0s1-20261104/final-evidence-index.json`

Index SHA-256:
`f7e9dab656cae8b8ade7135ae3674dfd2233c2a29eda9166f2ae9360390718a4`

## Local validation and blocker

Evidence run: `c-s0s1-20261104`.

- `cargo check --workspace --locked`: pass (`workspace-final-select.log`).
- `cargo test -p oo-connectors --features mysql --lib --locked`:
  152 passed, 2 ignored (`unit-final-select.log`).
- Existing MySQL contract tests and new offline checks: pass; opt-in live
  tests remain explicitly ignored in this ordinary run (`mysql-tests-final.log`).
- `cargo fmt --package oo-connectors -- --check`: pass
  (`fmt-final-select.log`); `git diff --check`: pass.
- Full `cargo test -p oo-connectors --features mysql --locked`: fails in
  `file_worker_parent` (3 pass, 5 fail; `crate-final.log`).
- Reproduced with `--no-default-features --test file_worker_parent
  -- --test-threads=1` (3 pass, 5 fail;
  `unrelated-worker-isolation.log`), excluding the MySQL adapter.
- On Darwin 27, setting `SO_RCVTIMEO` after the Unix-socket peer closes
  returns EINVAL despite readable buffered bytes. Independent Python and
  Rust probes reproduce this (`socket-timeout-repro.log`,
  `socket-patch-probe.log`).

The historical `coordinator-file-worker.patch` is retained in ignored
`evidence/c-commit-s2-20261104/`, not shipped with the fixture.
`git apply --check` passes. Its timeout/buffered-data/EOF behavior passes the
standalone Rust probe; the full crate has not been tested with that patch.
The coordinator applied the Darwin fix on integration as `65fe11714`.
Before rebase, the lane reproduced the failing tests on its original base
and committed under task-card rule 9. No Cargo.toml, Cargo.lock, engine interface, docs/specs/execution,
or other coordinator-owned file was changed.

Earlier failures are retained, including fixture TLS setup, absent
mysqlbinlog executable, binlog padding-bit parsing, EXPLAIN format on 9.5,
idle-versus-explicit-sleep accounting, and temporary-server readiness.
None are counted as passing evidence.

No owned containers or volumes remain. Production CDC/T2, schema-drift and
fault-injection acceptance remain follow-ups; no support-matrix grade is
changed.
