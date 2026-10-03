# Frozen design — current catalog guard

Protocol `oo.source.pgsql.snapshot/v1`. This file is the implementation spec. The coder copies it. This analysis did not edit product, contracts, or tests, and did not run a builder or PostgreSQL.

Product blobs at workspace tip `3479e440f0e4858983e55014e5f01a4fe733b40e` match the a121 review of `2b6dd57a0dc3d36e098268638c0364af247d07fb`:

| Path | Blob |
| --- | --- |
| `crates/oo-connectors/src/snapshot.rs` | `53aa65f239497d5cad451a2c518669a4e6ec770c` |
| `crates/oo-connectors/src/session.rs` | `5211832e3f9b6b297e6db21cfcd0e72d920d4704` |
| `crates/oo-connectors/src/sql.rs` | `07febf17b415abcb785644f60690c47939e0e50f` |
| `crates/oo-connectors/src/error.rs` | `16143c3f809af5f73810d49fa2051a92bef14472` |
| `contracts/source/pgsql/snapshot-owner-v1.md` | `6306ae1cc3c5ff84809c9150212a66844986ba9a` |

## Decision

A committed catalog change is invisible to `SELECT` from `pg_attribute` inside the owner's one `REPEATABLE READ` transaction. That is the a121 failure (`add_column_active_current_schema`, generation 41, `SnapshotFinished`). The data connection stays that one transaction. A second session, admitted before either socket, reads the current committed catalog at the points below. The data cut does not move.

Holding `ACCESS SHARE` on the user table for the whole owner lifetime would block `ALTER TABLE` instead of observing `schema_drift`. That substitution is rejected.

## What is proved

a121 attempt 6, container `42792620d012…`, oracle exit 101. Owner generation 41, pid 527, start `2026-10-03 00:24:36.888346+00`, admitted `id` attnum 1 and `label` attnum 2. `ALTER TABLE drift.item ADD COLUMN extra text` committed before the user-table probe. Admin catalog then showed `extra` at attnum 6 (type oid 25, nullable). Dropped attnums 3, 4, and 5 were already on that database. Probe token `int:5 text:pebble`, hash `a8c9bd7271a6370bfef4abd3bd55fbe4d24e35d4855b3221d98df17eb509271c`. Finish returned `SnapshotFinished generation=41 pid=527`.

Cause in the fixed blobs: `recheck` (`snapshot.rs` 1026–1049) calls `read_attributes` (1244–1266), which runs `SNAPSHOT_ATTRIBUTES` (`sql.rs` 334–341) on the data session. `attributes_match` (882–897) compares attnum, name, type oid, typmod, and not-null with the shape captured in that same transaction. The new row is invisible, so the owner is not poisoned. `finish` (457–471) returns `SnapshotFinished` when rollback and close succeed.

PostgreSQL 17, `SET TRANSACTION`: repeatable read means every statement sees only rows committed before the first query or data-modification statement of that transaction. <https://www.postgresql.org/docs/17/sql-set-transaction.html>

PostgreSQL 17, transaction isolation, Read Committed: two successive `SELECT`s in one transaction can see different committed data. <https://www.postgresql.org/docs/17/transaction-iso.html>

`SET TRANSACTION SNAPSHOT` copies an existing snapshot and is only valid for `REPEATABLE READ` or `SERIALIZABLE` before the first query. It cannot show commits newer than that snapshot. Same SET TRANSACTION page.

`GetCatalogSnapshot` is an internal backend snapshot, separate from `CurrentSnapshot` ("the only snapshot taken in transaction-snapshot mode"). <https://github.com/postgres/postgres/blob/master/src/backend/utils/time/snapmgr.c> No documented client SQL returns that snapshot. `pg_export_snapshot` exports the transaction snapshot, which is the stale one. The guard must not import it.

`ALTER TABLE` takes `ACCESS EXCLUSIVE` unless a subform says otherwise. `ADD COLUMN` is not one of those weaker subforms. A nullable `ADD COLUMN` without a volatile default does not rewrite the table. <https://www.postgresql.org/docs/17/sql-altertable.html>

`SELECT` takes `ACCESS SHARE`, which conflicts only with `ACCESS EXCLUSIVE`. A lock is held until transaction end. A lock taken after a savepoint is released if the transaction rolls back to that savepoint. The sentence is in the explicit-locking chapter; the upstream wording change is commit `b9d94670c1d9b9a6604bb5cd80f77e93aee4e776` in `doc/src/sgml/mvcc.sgml`. <https://www.postgresql.org/docs/17/explicit-locking.html> <https://git.postgresql.org/pg/commitdiff/b9d94670c1d9b9a6604bb5cd80f77e93aee4e776>

`pg_attribute.attisdropped` rows stay in the catalog and are ignored by the parser. Attnums of ordinary columns number from 1. <https://www.postgresql.org/docs/17/catalog-pg-attribute.html> a121's attnum 6 is a fact about that database, not a value to hardcode.

a121 also proved the open path's catalog reads do not block `ADD COLUMN` before the first user-table probe: the `ALTER` committed while generation 41 was open.

Read-only transactions disallow `INSERT`, `UPDATE`, `DELETE`, `MERGE`, `COPY FROM` into a non-temporary table, `CREATE`, `ALTER`, `DROP`, `COMMENT`, `GRANT`, `REVOKE`, `TRUNCATE`, and `EXPLAIN ANALYZE` / `EXECUTE` of those. The documented list does not include `SAVEPOINT` or `ROLLBACK TO SAVEPOINT`. <https://www.postgresql.org/docs/17/sql-set-transaction.html>

There is no source connection ledger. `PgBudget` stores timeouts, `max_rows`, and `max_bytes` only (`config.rs`). `PgConnector::connect` (`session.rs` 33–79) opens a socket with no slot count. The cap 4 and 50,000 rows/s are `REQ-SRC-03` (`specs/open-ontology-v0.1-spec.md` line 294), `docs/03-sources-and-ingestion.md` line 53, and `docs/12-configuration-reference.md` line 77. Startup, retry, reconciliation, control, and profiling count. This design adds the ledger. It does not raise 4 or 50,000.

`SESSION_GUARDS` (`sql.rs` 6–10) sets `statement_timeout`, `lock_timeout`, `idle_in_transaction_session_timeout`, and `default_transaction_read_only=on` on every `PgConnector::connect`. `BEGIN` (`sql.rs` 101) is `START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY`. SQLSTATE `55P03` maps to `budget` / `lock_timeout`. SQLSTATE `57014` maps to `budget` / `statement_timeout` (`error.rs` 198–199).

## What is a hypothesis

These follow from the documents above and were not executed in a121. The PG 17.10 matrix must show them. If the savepoint hypothesis fails, the coder stops and returns the incompatibility in `HANDOFF.md`. The coder does not leave the user-table lock held and call that detection.

1. `SAVEPOINT oo_probe_lock` and `ROLLBACK TO SAVEPOINT oo_probe_lock` succeed on the data session after `START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY`, and a later `SELECT` on that session still returns row versions from the snapshot taken at `capture`.
2. That rollback releases `ACCESS SHARE` on the probed user table, so a concurrent `ADD COLUMN` can commit before `finish`, while the repeatable-read snapshot still hides the writer's new row versions.
3. The client's `SELECT` from `pg_attribute` uses the transaction snapshot. a121 proves it is stale. `GetCatalogSnapshot` is the internal explanation, not a second measurement.

## Rejected mechanisms

| Mechanism | Why it is rejected |
| --- | --- |
| `READ COMMITTED` or any other isolation on the data session | Moves the data cut. Forbidden. |
| `COMMIT`, a second `BEGIN`, or reconnect on the data session | Appends or replaces the cut. `session.begin` already returns `nested_transaction`. |
| `SET TRANSACTION SNAPSHOT` / `pg_export_snapshot` on the guard | Copies the old snapshot. |
| `LOCK TABLE` or an owner-long `ACCESS SHARE` on the user table | Blocks `ACCESS EXCLUSIVE` DDL. The owner then finishes with `SnapshotFinished` because no committed drift exists. a121 required an observed `schema_drift`. |
| Connect-per-check, `dblink`, or `postgres_fdw` back to the same server | Extra backends outside the two reserved slots. |
| Relabel SQLSTATE `55P03` as `schema_drift` | An in-progress, uncommitted `ALTER` is not a committed catalog change. |
| Accept the same-transaction `pg_attribute` reread as the requirement | a121 receipt forbids that weakening. |

An uncommitted `ALTER` that already holds `ACCESS EXCLUSIVE` can make the data probe wait. The server returns `55P03` or `57014` under the existing guards. The fact stays `lock_timeout` or `statement_timeout`. The owner is poisoned. `finish` is not `SnapshotFinished`. The guard's catalog `SELECT` takes `ACCESS SHARE` on `pg_class` / `pg_attribute` / `pg_index` only. That mode does not conflict with the `ROW EXCLUSIVE` catalog locks used by `ALTER TABLE`, and it does not lock the user table.

## Connections

One snapshot owner holds two backends for its whole life:

| Session | Transaction | Role |
| --- | --- | --- |
| Data | One `REPEATABLE READ READ ONLY` | Unchanged cut: catalog capture, privilege and node recheck, key probe |
| Guard | One `READ COMMITTED READ ONLY` | Current committed catalog only. Each statement sees commits before that statement. |

Both use the existing `PgConnector::connect` path: `verify-full`, caller CA only, `application_name=oo-pg-source`, no startup options, `session_user = current_user`, tested version `17.10`, `SESSION_GUARDS`, and `admit_connected`. The guard's `server_version_num` must equal the data session's. A difference is `read_node`. An untested version uses the existing version fact.

The guard then runs `START TRANSACTION ISOLATION LEVEL READ COMMITTED, READ ONLY` once. Later observations are statements in that transaction, not new transactions. Release is `ROLLBACK` then close. The client never sends `COMMIT`.

Public `SnapshotRequest`, `SnapshotOwner::open`, `binding`, `read_probe`, `finish`, and `abort` keep their signatures. `SnapshotBinding` gains no field. The guard pid is not public. A test identifies it as the other `oo-pg-source` backend whose pid is not `binding.backend.pid`. `SnapshotOwner`'s `Debug` may show `guard_open: bool` and the data pid. It must not show the password, the CA PEM, driver options, or SQL text.

`PgSession::catalog` and `inspect_access` stay single-session and are not this owner.

## Admission, before any socket

Constants, not `PgBudget` fields. A config change cannot raise them in this slice.

```text
MAX_CONNECTIONS = 4
MAX_ROWS_PER_SEC = 50000
ROW_WINDOW = 1 second
MAX_TRACKED_SOURCES = 64
```

Process-local mutex map keyed by source id. Same process boundary as the existing generation set, which is not a cluster lease. One entry is the source id (at most 63 bytes), `in_use: u8`, `row_tokens: u32`, and `window_started: Instant`. Idle entries may be evicted when the map is full. If all 64 entries have `in_use > 0`, `try_reserve` fails closed with `budget` / `max_connections`. A poisoned mutex fails the same way and does not connect.

`try_reserve(source_id, n)` adds `n` to `in_use` only when `in_use + n <= 4`. `SnapshotOwner::open` calls `try_reserve(source_id, 2)` after the existing local refusals (generation 0, deadline already passed, config and type-policy identity) and before `reserve` of the generation and before either `connect`. Failure does not consume the generation and opens no socket.

`PgConnector::connect` reserves 1 itself for callers other than the owner (`catalog` tests, `live_pg`, `lossless_live`). The owner uses a crate-private `connect_reserved(permit)` that does not reserve again. Each `PgSession` owns one permit. The permit returns its slot only after `Connection::close` on the finish, abort, and fail-open paths. `Drop` of a permit also returns the slot, so a panic cannot hold the cap. `Drop` does not wait five seconds. The five-second bound is the caller's `pg_stat_activity` poll, unchanged: `CLEANUP_OBSERVATION_BOUND_MS = 5000`.

Cap interaction, with no exception:

| Held slots | `SnapshotOwner::open` |
| --- | --- |
| 0, 1, or 2 | Reserves 2 and may connect |
| 3 or 4 | `budget` / `max_connections` before any socket |

One open owner occupies 2 of 4 until both sessions are released. Two owners of one source fill the cap. A third fails before connect. `catalog()` on an already connected session does not take another slot. A separate `PgConnector::connect` does. Concurrent snapshot owners per source per process go from four possible sockets to two owners. The number 4 is unchanged.

Row bucket: capacity 50,000, refill 50,000 at each 1-second window boundary, no wait, no queue. `try_charge_rows` fails with `budget` / `max_rows_per_sec` when the charge does not fit. Deadline is checked before the charge. A failed charge sends no further SQL.

Charges on this owner:

| Point | Amount | When |
| --- | --- | --- |
| Guard observation | Sum of the statement limits below | Before the first guard statement of that observation |
| Data probe | 2 | Before the probe `SELECT` |
| Captured catalog | Tables + `SourceColumn` rows + primary-key columns actually retained | After `collect_catalog` returns, before the owner is published |

The catalog charge can overshoot by one `collect_catalog` response. That response is already bounded by `PgBudget.max_rows` (at most 100,000) and `max_bytes` (at most 32 MiB). If the charge does not fit, `fail_open` releases both sessions and both slots. A fixture catalog of a few dozen rows fits. This slice does not charge `PgSession::catalog` outside the owner. The full-load window defaults to null in `docs/12` and is not this path.

Guard observation limits, all static SQL, parameters bound:

| Statement | Limit charged |
| --- | --- |
| `SNAPSHOT_NODE` once per observation | 1 |
| `CURRENT_RELATION` per selected table | 1 |
| `CURRENT_ATTRIBUTES` per selected table | `admitted_columns + 1` |
| `CURRENT_PRIMARY_KEY` per selected table | 9 |

`admitted_columns + 1` and the node row are `i64` bind parameters where the statement has a `LIMIT`. `CURRENT_PRIMARY_KEY` uses the literal `LIMIT 9` (`MAX_KEY_COLUMNS + 1`). A result that hits the attribute limit is `schema_drift` and is not stored past that row. Each retained name longer than 63 bytes, or containing NUL, is `schema_drift` and is not kept. Total UTF-8 bytes retained for one observation must fit `budget.max_bytes` via the existing `charge` helper. The observation struct is dropped after the compare.

## SQL templates

Add these to `sql.rs` and to the `ALL` list. Identifiers in the statements are fixed. Schema and table names are `$1` and `$2`.

```text
BEGIN_CURRENT =
  START TRANSACTION ISOLATION LEVEL READ COMMITTED, READ ONLY

SAVEPOINT_PROBE = SAVEPOINT oo_probe_lock

ROLLBACK_PROBE = ROLLBACK TO SAVEPOINT oo_probe_lock

CURRENT_RELATION =
  SELECT c.oid::int8, c.relkind::text
  FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  WHERE n.nspname = $1 AND c.relname = $2

CURRENT_ATTRIBUTES =
  SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull
  FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  JOIN pg_catalog.pg_attribute a
    ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
  WHERE n.nspname = $1 AND c.relname = $2
  ORDER BY a.attnum
  LIMIT $3

CURRENT_PRIMARY_KEY =
  SELECT a.attnum::int4, a.attname::text, u.ord::int4
  FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  JOIN pg_catalog.pg_index i ON i.indrelid = c.oid AND i.indisprimary
  JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true
  LEFT JOIN pg_catalog.pg_attribute a
    ON a.attrelid = c.oid AND a.attnum = u.attnum
  WHERE n.nspname = $1 AND c.relname = $2
  ORDER BY u.ord
  LIMIT 9
```

`SNAPSHOT_NODE` is reused for the guard's principal, database, recovery, and postmaster. Privilege columns stay off the guard statements. `has_*_privilege` remains on the data session, where a121 showed it is not frozen by the snapshot.

Do not send `RELEASE SAVEPOINT`. Release would keep the locks. Do not add `COMMIT`.

## Typed compare

For every selected table, against `binding.catalog` and `binding.access`:

| Current result | Fact |
| --- | --- |
| Relation row count other than 1 | `schema_drift` |
| `oid` differs from the bound relation oid | `schema_drift` |
| `relkind` other than `r` | `schema_drift` |
| Attribute set differs in length, attnum, name, `atttypid`, `atttypmod`, or `attnotnull` | `schema_drift` |
| Primary-key `(attnum, name)` sequence differs from the bound key order | `schema_drift` |
| Guard node principal or database differs | `identity_drift` |
| Guard recovery or postmaster differs, or version_num differs | `read_node` |
| Guard column `SELECT` is not this check | unchanged privilege path on the data session |

Zero guard rows for a name that was admitted is `schema_drift`, not `selected_table_missing`. An index rename with the same key columns is not drift. A new relation oid is drift even when the column names match.

The data session's existing `recheck` stays. It still checks node identity, relation oid inside the snapshot, privileges, and the snapshot attribute set. That last compare will pass when the only change is a commit after the snapshot. It is not the current-catalog check. The guard compare is.

## State machine

```text
Refused
  local failure before try_reserve
  generation not consumed, slots 0, sockets 0

CapRefused
  try_reserve(2) failed
  generation not consumed, slots 0, sockets 0

Admitting
  generation consumed, 2 slots held
  data connect, data admit, guard connect, guard admit
  data BEGIN (RR), guard BEGIN_CURRENT
  capture on data, then guard observe of every selected table
  any failure -> release both, return that error, slots 0

Open
  both transactions open, poison empty

Open --read_probe--> Open
  deadline
  data recheck (node, privileges, snapshot shape)
  guard observe all selected tables
  SAVEPOINT_PROBE, probe SELECT, ROLLBACK_PROBE
  decode
  guard observe again
  return the batch only if both observes match

Open --read_probe error--> Poisoned
  includes schema_drift, privilege, identity, read_node, budget, probe SQL error
  if the probe SQL errors, still ROLLBACK_PROBE when the savepoint exists,
  then guard observe; committed drift replaces the probe error with schema_drift

Poisoned --read_probe--> Poisoned
  existing poison, no new SQL

Open --finish--> Finished
  deadline still in the future
  guard observe all selected tables, including when read_probe was never called
  match, rollback and close both, deadline still in the future
  SnapshotFinished

Open or Poisoned --finish, observe drift or error--> Closed error
  set poison if not set, release both
  return schema_drift or the earlier poison when release succeeds
  return the cleanup error when release fails

abort --> SnapshotAborted when both releases succeed
  cleanup failure is the error, not SnapshotAborted and not SnapshotFinished

drop --> both sessions dropped
  Tokio present: spawn rollback and close, as today, for each session
  no runtime: drop both sockets
  permits return on session drop
  neither success value is returned
```

`finish` observes before it rolls back. Rollback first would drop the guard transaction's ability to see a committed change only if the guard were the session being rolled back; the data rollback is what releases a leftover user-table lock. Order on the success path: guard observe, then release data, then release guard. On `schema_drift`: the data transaction may still hold no user lock (no-probe, or probe already rolled back to the savepoint). Observe sees the commit. Then release both. Do not return the batch from a probe whose post-observe failed.

A deadline that has already passed at the door of a step sends no SQL for that step. Release still runs. The returned fact is the existing poison, otherwise `deadline`. A drift observed while the deadline is still in the future stays `schema_drift`.

Generation reuse stays `generation_reused`. Failure after generation reserve consumes the generation. There is no second data transaction and no reconnect.

## Race matrix

| When the other session commits | What the owner does |
| --- | --- |
| Before the data snapshot's first statement, and the admitted shape matches the new catalog | Open continues. Both reads see the new shape. |
| After the data snapshot starts and before the open observe | Open returns `schema_drift`. No owner. |
| After open, before `read_probe`, `ADD COLUMN` on an unprobed table | Pre-observe returns `schema_drift`. The probe `SELECT` does not run. |
| During the probe `SELECT` | `ALTER` waits on `ACCESS SHARE`. It is not committed. Post-observe still matches unless something else committed. |
| After `ROLLBACK_PROBE` and before post-observe | Post-observe returns `schema_drift`. The batch is not returned. |
| After `read_probe` has returned, before `finish` | `finish` observe returns `schema_drift`. The earlier batch remains the old cut. Success is `finish`, so the run is not `SnapshotFinished`. |
| After the last statement, `finish` with zero probes | `finish` observe sees it. `schema_drift`. |
| `DROP` plus `CREATE` of the same name | New oid. `schema_drift` even if column names match. The data session is not retargeted. |
| Rename, type, typmod, `SET NOT NULL`, or primary-key column change | Attribute or key compare fails. `schema_drift`. |
| Uncommitted `ALTER` holding `ACCESS EXCLUSIVE` | Probe may return `lock_timeout` or `statement_timeout`. Not relabeled. |
| Privilege change only | Data-session `has_*_privilege` path. Unchanged facts. |
| Guard backend killed, or guard node/database/principal/version differs | Fail closed. Release both. Not `SnapshotFinished`. |

## Contract text the coder writes

Replace the schema sentences in `contracts/source/pgsql/snapshot-owner-v1.md`. Keep the data-cut sentences.

Open, in place of the sentence that defines `schema_drift` as drift from the catalog just captured in the same transaction:

```text
The attribute set and primary key are compared with the admitted shape inside this transaction. A mismatch is catalog_identity. A drift between that captured shape and a later current-catalog observation is schema_drift. The current observation is a second session in READ COMMITTED READ ONLY. It is not a second read of pg_attribute inside this repeatable-read transaction. The data connection remains the one repeatable-read read-only transaction. The second session is reserved against the source cap of 4 before either socket is opened.
```

Probe, keep the privilege paragraph. Replace the shape sentence with:

```text
A difference of relation oid, relkind, attnum, name, type oid, typmod, nullability, or primary-key attnum and name order, between the captured shape and the current committed catalog, is schema_drift. The owner is poisoned. finish is not SnapshotFinished. The same compare runs before the probe statement, after it, and on finish, including when no probe ran.
```

Finish, add:

```text
finish reads the current catalog before rollback when the deadline has not yet passed and the owner is not already poisoned. A mismatch is schema_drift. SnapshotFinished still requires accepted rollback and close of every session this owner opened, an empty poison, and a deadline that has not passed. Cap refusal before connect does not consume the generation. Failure after the generation is reserved does.
```

Facts to add to the fact list: `max_connections`, `max_rows_per_sec`. No new error kind. `SourceError::budget` already takes a static fact.

README sentence that says `open` connects once: say the owner holds one data connection and one current-catalog connection, and one repeatable-read read-only transaction on the data connection.

## Write paths

The implementation commit may change only:

| Path | Change |
| --- | --- |
| `crates/oo-connectors/src/session.rs` | Ledger, permit, `connect_reserved`, `begin_read_committed` |
| `crates/oo-connectors/src/sql.rs` | The six templates and the `ALL` list |
| `crates/oo-connectors/src/snapshot.rs` | Guard field, open order, observe, savepoint around the probe `SELECT`, finish/abort/drop of both sessions |
| `contracts/source/pgsql/snapshot-owner-v1.md` | The paragraphs above |
| `crates/oo-connectors/README.md` | The one-connection sentence |
| `crates/oo-connectors/tests/snapshot_live.rs` | Same-cut regression and the live cases the package test can host |

No manifest, lock, vendor, `error.rs` enum, docs, spec, plan, or ledger edit. No new dependency. `error.rs` needs no change if the new facts are passed to `SourceError::budget`.

Tests that call `PgConnector::connect` and drop the session keep working while they hold at most 4 sessions. A test that holds 4 and connects again receives `max_connections`. Fix that test by dropping a session. Do not raise the cap.

## Secrets and release

`connect_reserved` keeps `disable_statement_logging` and the existing `Debug` redaction. The password and CA PEM stay on `PgSourceConfig`. They are not copied into `SnapshotBinding`, the ledger, logs, or batches. Guard SQL parameters are schema names, table names, and limits.

Every path that has reserved slots or sockets releases both: open failure, probe error, finish, abort, drop, deadline, guard disconnect, TLS failure, database or node mismatch. `fail_open` grows to two sessions. A guard connect failure releases the data session that already connected. A data connect failure drops the unused second permit.

`oo-pg-source` on both backends is one audit actor. Allowed client statements for that actor: `SELECT`, `START TRANSACTION …`, `ROLLBACK`, `SAVEPOINT oo_probe_lock`, `ROLLBACK TO SAVEPOINT oo_probe_lock`, and the existing `set_config` selects. Zero `INSERT`, `UPDATE`, `DELETE`, `COMMIT`, `ALTER`, `CREATE`, `DROP`, `GRANT`, `REVOKE`, `TRUNCATE`, `COPY`, and `CALL`. A probe run must contain the savepoint pair, so the lock release is visible in the log. A no-probe run may omit it.
