# E-S5d: Oracle T3/T4 (2026-11-05)

Status: partial delivery with explicit downgrades, not unconditional AT-ENG-02
acceptance. No support-matrix or plan row is upgraded. The shared T3/T4 entry
point now accepts Oracle; the Oracle adapter supplies SQL/transport only and
uses the same fault triggers, durable-state assertions, and T4 state machine.

## Integration boundary

`E-S5d-shared.patch` is the exact, unapplied coordinator-owned oo-load change
connecting Oracle `table_shape` to C5 lineage. **All live results below use
this patch. The unpatched loader does not enforce Oracle shape lineage.**
The first binary was built with a temporary working-tree patch (subsequently
removed); the final binary was built from an ignored candidate archive with
the patch applied. No shared loader change or lockfile change is committed.

- Initial binary SHA-256: `0385828b7d6c8d9df5a058b802fb009a8274651ada8069874790d788942a1fea`.
- Final binary SHA-256: `b1d78878dc7d834a9d51e4b53ded18bbd6d78890423afd5bfc2dc385b3602a2c`.
- The only product change between these binaries is the LogMiner logging
  prerequisite guard. Oracle 21/11 runs use only C5, never this C1 path.

## Reproduce

Build the prerequisites before starting containers; preserve the resulting
binary if reclaiming this worktree's Cargo artifacts to meet the disk gate.
Apply the shared patch only in a coordinator-authorized checkout/candidate.

```sh
cargo fmt -p oo-connectors
cargo check --workspace --locked
cargo test -p oo-connectors --features oracle --locked --no-fail-fast
CARGO_TARGET_DIR="$PWD/target" cargo build --manifest-path tools/oo-load/Cargo.toml --locked
PYTHONPATH=tools/oo-test python3 -m unittest discover -s tools/oo-test -p test_t3t4.py
tools/oo-test/oo-test t3t4 --engine oracle --versions 23,21,11 \
  --run-id e-s5d-oracle-02 --binary <built-oo-load> \
  --worker-image oo-e-oracle-worker:e-fix-worker-v4 --disk-wait-minutes 0
tools/oo-test/oo-test t3t4 --engine oracle --versions 23 \
  --run-id e-s5d-oracle-03 --binary <final-oo-load> --disk-wait-minutes 0
tools/oo-test/oo-test t3t4 --engine oracle --versions 23 \
  --cases source_restart_mid_chunk,retention_expiry,resetlogs_incarnation \
  --run-id e-s5d-oracle-04 --binary <final-oo-load> --disk-wait-minutes 0
tools/oo-test/oo-test t3t4 --engine oracle --versions 23 \
  --cases source_restart_mid_chunk --run-id e-s5d-oracle-05 \
  --binary <final-oo-load> --disk-wait-minutes 0
```

Rust builds/tests use `CARGO_PROFILE_DEV_DEBUG=0` and
`CARGO_PROFILE_TEST_DEBUG=0`. Raw logs remain in ignored evidence directories;
`t3t4-evidence-summary.json` contains hashes and per-run results, not rows.

## Version results

Actual versions are independently present in snapshot receipts:
23.5.0.24.07, 21.3.0.0.0, 11.2.0.2.0. The earlier harness's banner field
truncated Oracle patch versions; the final harness reads version_full (11g:
version). Source and target images resolve through the existing digest lock;
11g uses the amd64 thick worker image resolved to its content ID.

| Case | 23.5 (C5 + explicit D11a C1) | 21.3 (C5) | 11.2 (thick C5) |
|---|---|---|---|
| add/drop/rename column | pass | pass | pass |
| widen NUMBER and VARCHAR2 | pass | pass | pass |
| add/drop table | pass | pass | pass |
| PK drop/add | pass | pass | pass |
| source restart mid-chunk | pass targeted run 05; prior failures retained | pass | pass |
| network drop mid-C5 | pass | pass | pass |
| network drop mid-C1 | pass | not requested | not requested |
| target restart | pass | pass | pass |
| SIGKILL mid-cycle | pass | pass | pass |
| redo/archive retention expiry | not run: RMAN absent | not applicable to C5 | not applicable to C5 |
| physical RESETLOGS/incarnation | pass (run 04) | pass (run 02) | pass (run 02) |

23 uses run 03 except RESETLOGS run 04 and source restart run 05. 21/11 use run 02. Refused T4 paths
rebaseline as new bindings and compare every column and row after additional
DML. New-table scope is expanded explicitly: both old and new tables must
compare equal. Oracle values use NULL-aware hex framing; no whitespace or
numeric rounding normalization hides divergence. C1 is never implicitly
selected on 11/21; on 23 the policy explicitly enables LogMiner and uses a
separate common-user mining connection.

## Product changes and observed refusals

- Fresh Oracle column/key shape includes names, types, nullability, byte
  length, collation, precision, scale and key shape. Row counts and row-size
  estimates are excluded. C5 metadata-only changes refuse with
  `c5_rebaseline_required_lineage`, including unchanged-row widening/rename.
- C1 DDL refuses with `logminer_table_rebaseline` or
  `sync_rebaseline_required_lineage_or_scope`; new C5 table scope currently
  returns the closed but unspecific `load_target`. Rebaseline equality is
  still checked, rather than treating any refusal as a finished case.
- Physical RESETLOGS uses a controlfile trace generated inside the owned
  source. DBID/resetlogs SCN is measured before and after, not forged in
  policy. Old C5/C1 state refuses without publication changes; new bindings
  reach zero divergence after DML.
- Recreating the controlfile clears minimal supplemental logging. The new
  miner guard checks ARCHIVELOG plus YES/IMPLICIT minimal logging on admission
  and each read. Cleared/unknown settings refuse by the closed fact
  `logminer_logging_prerequisites_rebaseline`. The live RESETLOGS test first
  proves that refusal, then restores logging through the admin connection
  as required by Oracle's generated recovery script.

## Preserved incidents and limits

- Run 01: RMAN command unavailable (exit 127); 21/11 blocked by the 20 GiB
  disk floor. The initial new-table case did not expand capture scope and
  is not used for that acceptance. Corrected scope passed in run 02 onward.
- Run 02: 23 C1 accepted an empty result after controlfile recreation had
  cleared minimal logging; three committed source rows were missing. This
  is a real fail-open prerequisite bug, not a successful RESETLOGS run. The
  logging guard fixes it; run 04 verifies refusal and zero-difference recovery.
- Run 03: the new logging refusal was correct, but the harness initially
  compared a bare fact against the full SourceError rendering. Run 04 fixes
  the expectation to the exact public error; original failed evidence stays.
- Runs 03/04: source restart resume fails with
  `source error: protocol fact=oracle_database_error`; the first interruption
  returns `dead_connection` or `connection_recovery_failed`. Runs 01/02 passed.
  No ORA number was exposed, so the underlying cause is not attributed. A
  final targeted run (05) checks reader-table readiness rather than SYS dual;
  it passes, with zero divergence and no duplicate receipts. This points to
  a readiness race, but does not identify the earlier ORA error. All prior
  failures remain failures, not retroactive passes.
- Physical archive expiry is not injected because the pinned slim image
  lacks RMAN even at `$ORACLE_HOME/bin/rman`. No fake SCN/floor or catalog
  edit substitutes for actual history loss. This is an explicit downgrade.
- Restart injection uses the existing chunk/position progress triggers; it
  is mid-load/publish work, not proof of the precise transaction COMMIT edge.
  No SSH jump, throttling, T7 audit, full type/scale, model, or public Gateway
  acceptance is claimed. Dropped-table old views are not garbage-collected.
- Baseline snapshots still do not record shape: metadata changes before the
  first C5 cycle remain the shared harness's documented gap.

## Validation and resource hygiene

Workspace checks pass. Final connector library: 167 passed, one ignored.
The complete crate test run has one pre-existing macOS failure:
`parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`
(`group_signal` rejected versus sent). The identical assertion is reproduced
on the unchanged rebased base archive, recorded in
`e-s5d-build-20261105/base-test.log`. No test was disabled or gold changed.
Python T3/T4 tests: 13 passed. Shared patch applicability and diff whitespace
checks pass.

Only owned containers/volumes are created and removed, all labelled
`oo.lane=e` plus their run ID. At most one source DB and one target DB run at
once; the optional 11g worker is client-only, on a separate network namespace
so dropping the source network affects it. The 20 GiB gate is retained and
also checked before starting the target. No external service, customer
system, shared worktree, token file, image, or volume is modified.

Final resource inspection: no containers or volumes for any recorded run ID
remain; no lane-created service is left running.
