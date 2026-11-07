# Lane C S2c: connector delivery

S2-C-01 and S2-C-02 are **author-complete at connector level**, not accepted
end to end. Agent DB durability, T1/`oo-load`, zero target rewrites and zero
new generations on no-change cycles remain pending lane M. No support
matrix grade is changed. The S2b commits remain on this rebased lane.

Code commits: `6bab5582b`, `fdab17311`. No shared files or lockfiles changed.
The coordinator's existing optional futures-util edge is used.

## Implementation

- `MysqlEngine::changes` returns a `ChangeSource` under the non-default
  `mysql-binlog` feature. It decodes ROW/FULL table maps, inserts, updates,
  deletes, key moves, transaction boundaries, ordinary and tagged GTIDs.
  A batch is one complete transaction. An oversized transaction returns a
  budget error without a partial checkpoint. File-position mode also
  handles anonymous transactions with GTID mode off.
- Every change has one aligned Arrow image, including a before image for
  deletes. Exact composite keys, unsigned integers, decimals, JSON,
  DATETIME, TIMESTAMP, negative TIME, binary/NULL values and zero YEAR are
  compared against snapshot images. JSON and TIMESTAMP formatting uses
  read-only scalar queries on the **logged value**, not a current-row read.
- `earliest_available` uses the actual binlog index.
  `MysqlChanges::retention` reads the oldest log's first dated event and
  server time, then rechecks the inventory and lineage. The repair path
  enforces the caller's `required_history_seconds` before full reading.
  Short/unmeasured history, missing logs and GTID lineage loss are explicit
  new-generation rebaseline requirements/errors, not implicit repair.
- `repair_to_c1` records L0 before the first full read and consumes changes
  concurrently with each chunk. Logged keys override snapshot rows;
  tombstones survive later chunks and key moves. Complete source/binding
  scope is repaired and drained to L_end. The result retains the original
  chunk receipts and schema-bearing empty batches for empty tables.
- `HandoffBarrier` requires every source and binding in its frozen scope
  to stage a repair and confirm the exact L_end before releasing any C1
  cursors. Confirmation is the loader's durable-application responsibility.
  The output is a vector of source positions, not a global snapshot.
- The existing empty-chunk adapter already met the contract. A live test
  now verifies one empty batch, its schema, and the exact IPC bytes/hash.

## Final real-engine evidence

All commands use `python3 testing/engines/mysql/run.py`:

| Suite | Run ID |
| --- | --- |
| `--suite s2-c1` | `c-s2c-c1-20261104-08` |
| `--suite s2-handoff` | `c-s2c-handoff-20261104-05` |
| `--suite s2-c5` | `c-s2c-c5-20261104-01` |

| Actual MySQL version | C1 three cycles | Concurrent repair / handoff | C5 cycles |
| --- | --- | --- | --- |
| 5.7.44, amd64 | PASS | PASS | PASS |
| 8.0.44 | PASS | PASS | PASS |
| 8.4.7 | PASS | PASS | PASS |
| 9.5.0 | PASS | PASS | PASS |

C1 cycles contain 4, 4 and 0 changes, with exact image equality. Additional
cases cover rollback, whole-transaction budget refusal/retry, actual purge,
actual GTID reset, file positions, anonymous transactions, tagged GTIDs on
8.4/9.5, empty IPC, and sufficient/insufficient actual retained history.

Each handoff test uses **two independent servers and eight bindings**,
starting from confirmed C5 baselines. A separate fixture writer waits for
the first real snapshot transaction in performance_schema, then writes
concurrently. Both sources must have a changed low/high chunk window.
Final staged rows equal independent SELECT results; deletions before L0,
window tombstones, key moves, incomplete barriers,
post-switch C1, staging exhaustion and retention refusal are exercised.
This is an in-memory connector oracle, not an Agent DB acceptance test.

Runtime version strings, image digests/architectures, source hashes and raw
outputs are retained in the ignored evidence directories. All final source
hashes match code head `fdab17311`. No lane-C containers or volumes remain.

## Local validation and preserved failures

Evidence directory: `evidence/c-s2c-20261104/`.

- `cargo check --workspace --locked`: PASS before each code commit.
- `cargo fmt -p oo-connectors -- --check`: PASS.
- Feature-enabled connector suite: **305 passed, 29 ignored**, excluding
  only `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`.
  An unfiltered run fails that test; the identical `group_signal` assertion
  is reproduced on archived unmodified base `7dec887e1` in
  `base-parse-deadline.log`. No test or gold value was weakened.
- Unit tests also reject foreign/duplicate repairs, missing bindings and
  confirmations with the wrong L_end.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: PASS.
- Staged secret, absolute-path and file-size scans: PASS.

Earlier failures remain available: missing fixture tagging privileges,
5.7 fixture charset, JSON whitespace mismatch, a 9.5 writer that missed the
actual chunk window, and a temporary compile error. The window assertion
was retained; fixture scheduling now observes the real snapshot session.

Raw index: `evidence/c-s2c-20261104/final-index.json`.
SHA-256: `dba551acb57aaf3d0ad62f378f6c4a69cda3785796ae6cc0656ae83af18cc40c`.

## Boundaries and follow-up

- Repairs are bounded in-memory proposals, not a durable log spool or a
  loader transaction. Budget exhaustion aborts; nothing is published.
- DDL, unsupported event/type shapes, partial row images and unsupported
  source charsets fail closed. This is not blanket MySQL feature coverage.
- Scalar formatting adds source round trips for JSON/TIMESTAMP cells.
- Lane M must persist complete replacements/tombstones/checkpoints, own
  generation transitions, and run its `oo-load`/T1 acceptance suite.
- A bare file/position is not a cross-instance identity proof; persist the
  source/generation binding with checkpoints. The tested reset detection
  uses GTID discontinuity, not an externally reconstructed file epoch.
- C1 remains opt-in per plan D10; default-artifact admission is unchanged.
- Requested shared-file patches: none. `Cargo.lock` changes: none.
