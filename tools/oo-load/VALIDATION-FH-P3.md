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

## Field-hardening fix lane A (review R1, R2, R4, R6, R8)

### Stale-reason grammar (R1, R8)

`crates/oo-pipeline/src/stale_grammar.rs` is the one grammar. The loader
produces it, `PgSync::quarantine` and `engine_isolation::record` refuse
anything else, the release coordinator validates it, and the Gateway parses
it. The coordinator and the Gateway compile the same file with `#[path]`.

```text
reason = code / "schema_drift:" column ":" kind
code   = schema_unproven / schema_drift / rebaseline_required / source_changed
       / source_permission / source_timeout / table_budget
column = "unpublished_column" / "c." enc / "h." 16 lowercase hex
kind   = dropped / renamed / nullability / type / key / collation / ordinal
```

`enc` keeps ASCII letters, digits and `_` as they are and writes every other
UTF-8 byte as lowercase `%xx`, so `PAYLOAD`, `userId`, `a:b` and non-ASCII
names all round-trip. A reason is at most 192 bytes, which is the existing
ledger bound, so no migration is needed. A name whose encoding would not fit
(for example 63 bytes of two-byte characters) becomes `h.` followed by the
first 16 hex digits of the SHA-256 of the name. Only published projection
columns are ever named. A key column outside the projection and a new key
column both report `schema_drift:unpublished_column:<kind>`. The Gateway shows
a published name decoded. Any other `schema_drift:` text, such as an older raw
row, is reduced to the generic `schema_drift`, so an unpublished name never
reaches an Agent.

### Per-table outcomes from the current cycle (R2, R6)

`oo-load publish --generation ANCHOR --outcome-run CURRENT` replays ANCHOR
only if CURRENT is a completed cycle over the same scope that recorded the
same per-table health: the same retained run, stale or not, the same reason
and the same stale interval. Fresh and no-op count as the same health.
Otherwise the cut is minted at CURRENT, which keeps each table's retained
data from its retained run. The candidate reports `run`. A fresh outcome must
carry the cut's own run (`publication_outcome_run`).

The lite scheduler always passes the current run. If there is no fresh table
and no single retained run, it uses the active anchor, or the current run when
there is no anchor. It advances `data_verified_at` only when the cut lists
exactly this cycle's tables and every table it calls fresh/noop was healthy in
this cycle, and every table it calls stale was stale in this cycle.

The release coordinator requires `stale_tables` to match the stale rows one
to one, with closed `{schema,name}` entries. It also requires fresh rows to
carry `retained_run == run_id`.

### Upgrade from the pre-0129 layout (R4)

Targets built before migration 0129 have no `_oo_load.column_catalog`. Their
C5 lineage still includes the table's catalog shape: MySQL/Oracle store
`<epoch>#shape:<fingerprint>`, and PostgreSQL stores a third element per
relation with columns, types, nullability and primary key. Without an upgrade
path, every table classified `schema_unproven` and every cycle exited 4.

On the first post-upgrade C5 cycle, a table with no catalog is checked
against its legacy lineage. The loader recomputes that lineage from the live
source. If it equals the stored value byte for byte, the old fence proves the
shape is unchanged. In one target transaction the loader then seals the
catalog from the live column facts. The projection is the current
generation's own columns, and each must exist live with the same target type.
The same transaction rewrites the C5 state lineage to the current format, and
the receipt shows `"schema_upgrade":"legacy_lineage_sealed"`. If the catalog
already exists but the lineage is still in the legacy format, the same proof
rewrites only the lineage (`legacy_lineage_rewritten`). Any difference leaves
the table `schema_unproven` or `rebaseline_required`, and the loader never
guesses. PostgreSQL C1 uses the same proof against the durable
`(lineage, slot, publication)` binding, read before `PgSync::open` drops the
shape element. MySQL/Oracle C1 lineage never carried a shape, so those tables
still need an explicit rebaseline after an upgrade. Collation was not part of
the legacy fence and is trusted from the live facts at sealing time.

`tools/oo-load/test-upgrade.py --old-binary <oo-load built at b59356412>`
covers this. It builds the target with the old binary (baseline plus first
C5, and PostgreSQL C1), then runs the current binary once. The cycle exits 3:
the two unchanged tables (one with new rows) are healthy and sealed, and a
table whose nullability changed behind the legacy fence stays
`schema_unproven`. The next cycle is a plain no-op. On PostgreSQL C1, both
tables are healthy and sealed after one cycle.
