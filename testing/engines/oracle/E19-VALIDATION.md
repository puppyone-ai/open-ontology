# Oracle 19c acceptance (lane E19)

Owner completed Oracle account, registry terms and Docker login: E365.
No account creation or license acceptance was performed by this executor.
`19.19.0.0` resolves to the native `linux/arm64` manifest
`sha256:3843234f6fd1ed084b1dd7ad42eeeaa95ad13f12c790810cb368bb58f8ad42ba`.
Measured database version: `19.19.0.0.0`. Registry inspection is retained in
`evidence/e19-resume-20261105/manifest.txt`; the earlier registry config
inspection records `architecture=arm64`. No amd64 emulation was needed.

## Commands and verdicts

Build prerequisites:

```sh
cargo check --workspace --locked
cargo build --locked -p oo-cli
CARGO_TARGET_DIR="$PWD/target" cargo build --locked --manifest-path tools/oo-load/Cargo.toml
```

The commands below run serially, with at most two E19 database containers.
All resources carry `oo.lane=E19`; fixture mutations use the separate admin
connection. Product reads use the restricted reader/common miner accounts.

```sh
tools/oo-test/oo-test matrix --engine oracle --versions 19 --suites T0,T1,T8 \
  --hook T0=tools/oo-test/t0.py --hook T1=testing/engines/oracle/e19_t1.py
python3 tools/oo-test/t2_p0.py --engine oracle --versions 19 \
  --run-id e19-t2-final-20261105
tools/oo-test/oo-test t3t4 --engine oracle --versions 19 \
  --run-id e19-t3t4-final-20261105
python3 testing/engines/oracle/e19_t7.py --engine oracle --versions 19 \
  --kinds snapshot --run-id e19-t7-snapshot-final-20261105
python3 testing/engines/oracle/e19_t7.py --engine oracle --versions 19 \
  --kinds incremental --run-id e19-t7-incremental-final-20261105
tools/oo-test/oo-test matrix --merge-only \
  --merge-evidence evidence/h-s5-20261105T123929358107Z-9b3590c1-9176-47c4-a281-4e90e20a2b50 \
  --merge-evidence evidence/e19-t2-final-20261105 \
  --merge-evidence evidence/e19-t3t4-final-20261105 \
  --merge-evidence evidence/e19-t7-snapshot-final-20261105 \
  --merge-evidence evidence/e19-t7-incremental-final-20261105
```

The E19 T7 entry invokes `tools/oo-test/t7.py` in-process; lane T's runner
and classifier remain unmodified. The adapter supplies standard common-user
auditing, corrects the standard audit identity column, reassembles SQLPlus
line wrapping with exact SQL-length validation, and includes C1 coverage.
It never relaxes classification or removes rejected audit records.

| Version | Suite / variant | Result | Evidence run |
|---|---|---|---|
| 19.19.0.0.0 | T0 product doctor | pass | `h-s5-20261105T123929358107Z-9b3590c1-9176-47c4-a281-4e90e20a2b50` |
| 19.19.0.0.0 | T1 / T8 full snapshot | pass | same full-scale run |
| 19.19.0.0.0 | T2 / T8 C5 and C1 | pass | `e19-t2-final-20261105` |
| 19.19.0.0.0 | T3 / T4 C5 and C1 | pass | `e19-t3t4-final-20261105` |
| 19.19.0.0.0 | T7 snapshot | pass | `e19-t7-snapshot-final-20261105` |
| 19.19.0.0.0 | T7 C5 and C1 | pass | `e19-t7-incremental-final-20261105` |

T1 uses 2,000,000 events, 200 small tables and all 231 discovered fixture
tables. All comparisons are equal after concurrent writes, correction and
replay. Sampled source session peak is one; invocation duties are 7.08%,
4.11% and 2.96%, below the unchanged 25% budget.

T2 uses two keyed numeric/text tables, three C5 and three explicitly opted-in
C1 cycles. Long-transaction SCN precedes the baseline, the uncommitted row
is absent from the baseline, and LogMiner subsequently recovers it. Idle
cycles preserve target relation size, write zero target bytes and create
zero generations. Implicit LogMiner is refused, and changing the retained
transaction floor is refused without altering durable target state.
C5 session peak is one; C1 peak is two (reader plus common miner).

T3/T4 pass all 14 applicable cases, including source/target restart,
network loss, SIGKILL, resetlogs incarnation and seven schema-change cases.
PostgreSQL slot invalidation and MySQL GTID purge are not applicable.
Four log switches followed by RMAN `DELETE FORCE NOPROMPT ARCHIVELOG ALL`
cause `logminer_resume_floor_rebaseline`, leave the target unchanged and
require a successful new baseline with exact equality.

T7 snapshot covers 48,547 statements and incremental covers 8,969 statements,
both with zero rejected records. C1 remains
explicit opt-in under D11a; LogMiner-owned internal DML is separately
attributed, not described as literal database-wide zero writes. Snapshot
audit uses 50,000 events and 20 small tables; full scale is the separate T1
run. T2/T3/T4 are compact-fixture acceptance, not full type/scale proof.

## Validation and retained failures

`cargo fmt -p oo-connectors`, locked workspace check, CLI/load builds, 126
shared Python tests and five E19 adapter tests pass. Oracle Python test files
pass when discovered separately. The three spec/plan checkers pass.
`cargo test --locked -p oo-connectors --no-fail-fast` has one failure:
`parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`.
Its macOS process-group result (`rejected` instead of `sent`) is identical
on original base `85cdd4715`; both outputs are retained in
`evidence/e19-20261105/` and `evidence/e19-resume-20261105/`.
All other non-ignored tests in that invocation pass; ignored live-engine
tests are not claimed as executed.

Failed history remains intact:

- Original full-scale T1 passed but T8 failed on Enterprise PX sessions.
  Product SELECTs now request serial execution without source settings writes.
- `e19-t2-resume-20261105`: C5 passed; the eight-second fixture transaction
  committed before first post-restart baseline completed. A 60-second hold
  on 19c preserves the original overlap assertion; the fresh run passes.
- `e19-t3t4-resume-20261105`: physical archive expiry worked, but the harness
  rejected the typed error prefix. Only prefix normalization changed; the
  closed refusal list, unchanged target and successful rebaseline remain
  mandatory. A full fresh T3/T4 run passes.
- `e19-t7-resume-20261105`: snapshot collection failed on SQLPlus line
  wrapping; C5/C1 audit passed with 8,969 statements and no rejected records.
  Raw audit output is preserved, not replaced by the corrected replay.

The historical merge `h-merged-306c9899-bf53-4ebd-b5dc-1198a74859b9` intentionally
returns exit 1 because it includes that failed snapshot row; its latest
14 suite/variant verdicts are all pass. The final acceptance merge and
artifact hashes are in `e19-evidence-summary.json`.
The final merge `h-merged-07c7e204-8141-463f-8430-4c74bdd0079a` returns exit 0:
19 explicit rows, all pass, covering 14 unique suite/variant pairs. E19
container and volume inventories are empty after cleanup.

No Cargo.lock or coordinator-owned docs/specs/execution/support-matrix edits.
No shared-file patch is required. T5/T6, other Oracle patches/platforms,
production/customer systems and global matrix promotion are outside this
card. The branch was rebased onto integration commit `14dc87974` on resume;
subsequent integration-branch changes are not part of these tested binaries.
