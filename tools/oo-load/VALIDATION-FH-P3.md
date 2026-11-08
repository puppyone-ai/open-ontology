# FH-P3 handoff

Status: complete for the card exits on PostgreSQL 17, MySQL 8.4 and Oracle 23.
S6-M-06/S6-M-07 acceptance remains the coordinator's decision. Base: `993d1162a8cbbb13aae5d4d7ee156477c346d708`.
Raw evidence (git-ignored): `evidence/fhp3c-20261107/` and the
`fhp3c-final3-*` engine run directories. Earlier development runs from this
pass (`evidence/fhp3-*`, `evidence/fhp3c-20261107/attempt*`) are kept, not
overwritten.

## C1 semantics as implemented

- Migration 0130 persists each C1 table's position, quarantine reason and
  original stale timestamp (`_oo_load.sync_table_positions`). Legacy
  all-or-nothing bindings inherit their existing durable source position, not
  the current source head.
- Every cycle first classifies every table with `oo_load::schema::classify`,
  the C5 classifier, against the sealed column catalog: additive and
  unpublished_only pass (new columns stay outside the projection); proven
  same-target widening passes with no new generation; changed-target widening
  rebuilds that table into one complete aligned generation; other published
  changes are `schema_drift:<column>:<kind>`; no seal or no native shape is
  `schema_unproven`. Access and timeout keep `source_permission` /
  `source_timeout` after one retry.
- Quarantine commits before capture excludes a table. A mixed batch skips
  quarantined tables and advances only healthy table positions; row changes,
  tombstones and the shared cursor still commit atomically. The quarantined
  table's position, rows and published generation do not move.
- PostgreSQL slot feedback is capped by the minimum durable table position.
  MySQL/Oracle do not acknowledge log deletion; their retention checks remain.
- Restored access or a fixed schema does not resume a table from the healthy
  cursor. Rejoin reads a complete bounded snapshot between a low and a high
  source watermark, replaces every key changed in that commit-ordered log
  interval, re-verifies source epoch, shape and retention, and switches the
  replacement storage, local view, column seal, empty overlays and the table
  position (= high watermark) in one target transaction. Failure keeps the old
  data and the quarantine. Old overlays are never reused; earlier immutable
  publication generations are not rewritten.
- A rejoined table can lead the shared cursor; readers exclude its covered
  history before decoding and the target excludes it again before applying.
- Receipts, the isolation/health ledger and exit codes are the C5 contract:
  0 healthy, 3 partial, 4 all stale; unknown, instance-wide and target failures
  exit 1. Reusing a completed run ID replays its outcome.
- MySQL admits only a closed set of statement events: classified column
  clauses justified by native before/after facts, already-covered boundaries,
  quarantined tables, and row-neutral account management (GRANT, REVOKE,
  CREATE/ALTER/DROP/RENAME USER or ROLE, SET PASSWORD, SET DEFAULT ROLE, FLUSH
  PRIVILEGES) outside a transaction with pending rows. Any other statement,
  including one with a leading comment, still fails closed.
- Oracle's cleared-logging refusal now precedes any target write.
- Oracle tables or schemas with identifiers over 30 bytes are
  `schema_unproven` on C1: online-catalog LogMiner on 23ai returns
  `OPERATION=UNSUPPORTED, STATUS=2` for ordinary DML on them
  (`evidence/fhp3-oracle-long-name-diagnostic/`, with a short-name control in
  `evidence/fhp3-oracle-status-diagnostic/`).

These are local intake and owner-side health semantics, not a public Gateway
or customer acceptance claim. `source_time` remains unmeasured.

## Harness changes (assertions unchanged)

- `t9_isolation.py` runs every T9 case on C1 too, plus
  `C1-T1-quarantine-rejoin`: a fixture-owner writer runs
  insert/update/key-move/delete across a quarantine and the rejoin; the case
  requires a frozen table position while quarantined, nonzero log events in
  the repair's correction window, and exact source/target equality. 20000
  quarantined rows make the snapshot span real time so the writer overlaps it.
- Widening checks sum all drained receipts and count physical publications.
  C1 unpublished_only checks the receipt of the cycle that saw the drop.
- Strict T4 fixtures are now `t4_<case>_<path>` (was
  `t4_isolation_<case>_<path>`), so Oracle names stay within 30 bytes.

## Final results

Frozen binary (oo-load, dev profile, `--features mysql-binlog`) SHA-256
`7b928af77839667a08acf28c48e495e4e1ee8aa564287394462811dedad71e0d`, built at
`a9c0a76`. Every engine run below used it. Later commits change only the T1
writer cap (harness) and a clippy-only `matches!` rewrite of the MySQL
account-statement predicate (same patterns, unit-tested).

| Command | Result |
|---|---|
| `cargo check --workspace --locked` | pass |
| `cargo test --locked -p oo-pipeline --lib` | 268 pass, 15 ignored |
| `tests/engine_sync_quarantine.rs` on an owned disposable PG 17 target | 1 pass |
| `cargo test --locked --manifest-path tools/oo-load/Cargo.toml` | 44 pass, 7 ignored |
| Same with `--features mysql-binlog` | 45 pass, 7 ignored |
| `cargo test --locked -p oo-connectors --all-features --lib` | 335 pass, 7 ignored |
| `cargo clippy --workspace --locked --all-targets -- -D warnings` | pass |
| `cargo clippy --locked -p oo-connectors --all-features --all-targets -- -D warnings` | pass |
| `cargo fmt --all -- --check`, oo-load fmt check, py_compile | pass |

Engine runs (sequential, at most two `oo.lane=fhp` containers, none left):

| Engine | T9 (C5 + C1 + T1) | Full T3/T4 pass/fail/not-run/N/A |
|---|---|---|
| PostgreSQL 17 | 23/23, `fhp3c-final4-t9-postgres-20261107` | 15/0/0/2, `fhp3c-final4-t3t4-postgres-20261107` |
| MySQL 8.4 | 23/23, `fhp3c-final4-t9-mysql-20261107` | 15/0/0/2, `fhp3c-final4-t3t4-mysql-20261107` |
| Oracle 23 | 23/23, `fhp3c-final4-t9-oracle-20261107` | 14/0/1/2, `fhp3c-final3-t3t4-oracle-20261107` |

The five strict T4 cases (add_column, drop_column, rename_column,
widen_type, widen_string) pass on both C5 and C1 on all three engines. C1 T9
cases passing on all three: T9-01 permission, T9-02 timeout, T9-03 additive,
T9-03 unpublished_only, T9-04 string widening (0 generations), T9-05
changed-type widening (exactly 1), T9-06 drop/rename/nullability drift, T9-07
missing catalog, T9-11 unavailable source. Oracle retention_expiry is
not_run (no RMAN in the pinned slim image).

C1-T1-quarantine-rejoin (writer iterations / correction-window log events /
final rows equal): PostgreSQL 200 / 9 / 20002+2; MySQL 83 / 3 / 20003+3;
Oracle 200 / 3 / 20002+2. The quarantined table position stayed frozen while
the shared cursor advanced, and the rejoined table ended with no gap and no
duplicate.

Superseded attempts kept for audit: `attempt1-*` (PG T1 window had 0 log
events; MySQL refused GRANT/REVOKE events), `attempt2/` (Oracle
resetlogs guard changed target state; one Docker daemon disconnect),
`final3-*` (PG T1 writer backlog exceeded the bounded drain). Each led to the
commit named in its message.

## Integration and limits

- `FH-P3-INSTALLER.patch` is the requested install-retained-target change
  (0120 then 0130 on fresh and existing targets); not applied. Runtime
  `PgSync::open` installs 0130 idempotently.
- No root manifest, Cargo.lock, existing migration, installer, deployment,
  Gateway, spec or execution record was changed.
- Secret-review-bound files changed (ledger not self-approved):
  `tools/oo-load/src/main.rs`, `tools/oo-load/src/log_sync.rs`. The new
  `crates/oo-pipeline/tests/engine_sync_quarantine.rs` reads a runtime fixture
  password file and needs scanner classification.
- Complete-table recovery is bounded by the row/byte budget; it is not an
  unbounded large-table repair.
- PostgreSQL quarantine requires explicit publication members; all-table or
  schema publications and partition expansion fail closed.
- On MySQL/Oracle, if retention passes a table that stays quarantined (still
  denied or drifted), the cycle fails source-wide with
  `sync_rebaseline_required_retention` rather than isolating that table.
- On a fresh binding, a PostgreSQL/MySQL source-wide refusal raised while
  building the change reader can still follow the initial position write;
  the Oracle logging case is fixed, the others are not covered by a test.
- Oracle long identifiers are refused (`schema_unproven`), not supported.
  Oracle retention_expiry is not run (the pinned slim image has no RMAN).
- Other engine versions, arbitrary concurrent DDL and Gateway reader
  acceptance are not established by this matrix.
